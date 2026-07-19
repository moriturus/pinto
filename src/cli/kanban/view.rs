//! The pure Kanban view model ([`BoardView`]): selection, expansion, scrolling,
//! and column layout, with no dependency on the terminal or drawing.

use super::layout::{DisplayRow, PopupContent, board_items, column_display_rows};
use crate::cli::dependency_display::DependencyIndex;
use pinto::backlog::{BacklogItem, ItemId};
use pinto::error::Error;
use pinto::service::{Board, BoardColumn, BoardQuery, ReorderTarget, SearchFilter, SearchMode};
use pinto::timezone::DisplayTimezone;
use std::collections::HashSet;

/// In-progress search input shown at the bottom of the Kanban view (vim-style prompt).
///
/// Held only while the user is typing a query; committing or cancelling clears it. The applied
/// filter ([`BoardView::search`]) is untouched until the input is committed, so cancelling restores
/// the previous view without a reload.
struct SearchInput {
    /// Whether the pending query is interpreted as a substring or a regular expression.
    mode: SearchMode,
    /// Characters typed so far (empty commits as "clear the filter").
    buffer: String,
    /// Inline validation error (e.g. an invalid regex) surfaced under the prompt until the query changes.
    error: Option<String>,
    /// Filter applied when the prompt opened, restored on cancel. Incremental (substring) search
    /// edits the applied filter live, so cancelling must roll the board back to this snapshot.
    restore: Option<SearchFilter>,
}

/// Input form currently shown in the Kanban footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputMode {
    /// First step of the add form.
    AddTitle,
    /// Second step of the add form.
    AddBody,
    /// Third step of the add form.
    AddParent,
    /// Final relationship step of the add form.
    AddDependencies,
    /// Dependency target entry for adding a link.
    DependencyAdd,
    /// Dependency target entry for removing a link.
    DependencyRemove,
    /// Parent ID entry; submitting an empty value clears the parent.
    Parent,
}

/// A completed step from the Kanban input form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InputSubmission {
    /// The title was accepted and the form advanced to the body step.
    AddTitle { title: String },
    /// The add form advanced without performing persistence.
    AddStep,
    /// The title, body, and relationships are ready to be persisted.
    Add {
        title: String,
        body: String,
        parent: Option<ItemId>,
        depends_on: Vec<ItemId>,
    },
    /// A dependency operation is ready; the target is parsed by the runtime using the same ID
    /// parser as the CLI.
    Dependency {
        source: ItemId,
        dependency: String,
        remove: bool,
    },
    /// A parent assignment; `None` clears the current parent.
    Parent {
        source: ItemId,
        parent: Option<String>,
    },
}

/// Validation error raised before a form can advance.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InputValidation {
    /// The add form requires a non-blank title.
    EmptyTitle,
    /// Dependency operations require a non-blank ID.
    EmptyDependency,
    /// A typed relationship ID uses the same parser as CLI commands.
    InvalidItemId(Error),
}

struct FormInput {
    mode: InputMode,
    buffer: String,
    title: Option<String>,
    body: Option<String>,
    parent: Option<ItemId>,
    depends_on: Vec<ItemId>,
    source: Option<ItemId>,
    selection_anchor: Option<ItemId>,
    error: Option<String>,
}

/// Minimum display width of one column (including 2-column frame).
///
/// If there is room, the column will be expanded beyond this width to fill the drawing area. If there are many columns that require this width,
/// Keep each column legible by scrolling horizontally instead of reducing its width.
pub(crate) const MIN_COLUMN_WIDTH: u16 = 24;

/// Kanban display state (pure view model).
///
/// [`Board`] plus selection (`col`/`row`) and horizontal scroll (`col_offset`) state. It has no
/// dependency on terminal I/O or drawing.
pub(crate) struct BoardView {
    /// Board columns rendered by this view.
    board: Board,
    /// Full board for cross-column metadata such as dependencies and children.
    full_board: Board,
    /// Columns rendered by this view, in workflow order.
    display_statuses: Vec<String>,
    /// Selected column index (subscript of `board.columns`).
    col: usize,
    /// **Visible row** index within the selected column (subscript of [`Self::visible_rows`]).
    row: usize,
    /// Column index to display on the left (horizontal scroll position).
    col_offset: usize,
    /// Parent PBIs currently expanded; collapsed by default and preserved across columns and reloads.
    expanded: HashSet<ItemId>,
    /// Temporary status (such as WIP overage warning) to display in the footer. `None` displays key guidance.
    status_message: Option<String>,
    /// Whether focus mode (maximizes the selected column to fill the width of the terminal) is enabled.
    maximized: bool,
    /// Whether to render PBI bodies as Markdown in the details popup.
    render_markdown: bool,
    /// Whether the details popup is open.
    popup_open: bool,
    /// Scroll position of the popup body (number of lines from the beginning).
    popup_scroll: u16,
    /// Whether the Kanban help window is open.
    help_open: bool,
    /// Scroll position of the help window body.
    help_scroll: u16,
    /// Item query used when loading and reloading the board.
    query: BoardQuery,
    /// Pending in-view search input (vim-style bottom prompt). `None` outside search entry.
    search_input: Option<SearchInput>,
    /// Pending add/relation input. `None` outside a form.
    form_input: Option<FormInput>,
    /// Timezone used by human-readable timestamps in the details popup.
    timezone: DisplayTimezone,
}

impl BoardView {
    /// Create the initial state (first column, first row, zero scroll, all collapsed) from a board.
    /// Empty boards are handled safely.
    #[cfg(test)]
    pub(crate) fn new(board: Board) -> Self {
        let display_statuses: Vec<_> = board
            .columns
            .iter()
            .map(|column| column.status.to_string())
            .collect();
        Self::new_with_scope(board.clone(), board, display_statuses)
    }

    /// Create a view with a display-scoped board and a full board for cross-column metadata.
    #[cfg(test)]
    pub(crate) fn new_with_scope(
        board: Board,
        full_board: Board,
        display_statuses: Vec<String>,
    ) -> Self {
        Self::new_with_scope_and_query(board, full_board, display_statuses, BoardQuery::default())
    }

    /// Create a view with a display-scoped board and the query used to load it.
    pub(crate) fn new_with_scope_and_query(
        board: Board,
        full_board: Board,
        display_statuses: Vec<String>,
        query: BoardQuery,
    ) -> Self {
        let mut view = Self {
            board,
            full_board,
            display_statuses,
            col: 0,
            row: 0,
            col_offset: 0,
            expanded: HashSet::new(),
            status_message: None,
            maximized: false,
            render_markdown: true,
            popup_open: false,
            popup_scroll: 0,
            help_open: false,
            help_scroll: 0,
            query,
            search_input: None,
            form_input: None,
            timezone: DisplayTimezone::Local,
        };
        view.clamp();
        view
    }

    /// Toggle focus mode, which displays only the selected column at full width.
    pub(crate) fn toggle_maximize(&mut self) {
        self.maximized = !self.maximized;
    }

    /// Set the initial focus-mode state.
    pub(crate) fn set_maximized(&mut self, maximized: bool) {
        self.maximized = maximized;
    }

    /// Return whether focus mode is enabled.
    pub(crate) fn is_maximized(&self) -> bool {
        self.maximized
    }

    /// Set whether the details popup renders PBI bodies as Markdown.
    pub(crate) fn set_render_markdown(&mut self, render_markdown: bool) {
        self.render_markdown = render_markdown;
    }

    /// Return whether the details popup renders PBI bodies as Markdown.
    pub(crate) fn render_markdown(&self) -> bool {
        self.render_markdown
    }

    /// Set the timezone used by human-readable timestamps in the details popup.
    pub(crate) fn set_display_timezone(&mut self, timezone: DisplayTimezone) {
        self.timezone = timezone;
    }

    /// Timezone used by the details popup.
    pub(crate) fn display_timezone(&self) -> DisplayTimezone {
        self.timezone
    }

    /// Select columns by state name. If not found, return `false` without changing the selection.
    pub(crate) fn select_column(&mut self, status: &str) -> bool {
        let Some(column) = self
            .board
            .columns
            .iter()
            .position(|column| column.status.to_string() == status)
        else {
            return false;
        };
        self.col = column;
        self.row = 0;
        self.clamp();
        true
    }

    /// Open the details popup for the selected PBI. Do nothing when no item is selected.
    pub(crate) fn open_popup(&mut self) {
        if self.selected_item().is_none() {
            return;
        }
        self.popup_open = true;
        self.popup_scroll = 0;
    }

    /// Close the details viewing popup. The selected position does not change.
    pub(crate) fn close_popup(&mut self) {
        self.popup_open = false;
    }

    /// Is the details viewing popup open?
    pub(crate) fn is_popup_open(&self) -> bool {
        self.popup_open
    }

    /// Scroll position of the popup body (number of lines from the beginning).
    pub(crate) fn popup_scroll(&self) -> u16 {
        self.popup_scroll
    }

    /// Restart the popup body at the top. Used after an in-popup edit refreshes the shown item, so
    /// the (possibly shorter) new content is not left scrolled past its end.
    pub(crate) fn reset_popup_scroll(&mut self) {
        self.popup_scroll = 0;
    }

    /// Scroll the popup body by `delta` lines (rounding less than 0 and more than `max`).
    pub(crate) fn scroll_popup(&mut self, delta: i32, max: u16) {
        let current = i32::from(self.popup_scroll);
        let next = (current + delta).clamp(0, i32::from(max));
        self.popup_scroll = u16::try_from(next).unwrap_or(0);
    }

    /// Display content of open popup (`None` if closed).
    pub(crate) fn popup_content(&self) -> Option<PopupContent> {
        if !self.popup_open {
            return None;
        }
        let item = self.selected_item()?;
        let deps = self.dependency_index();
        let children = board_items(&self.full_board)
            .filter(|it| it.parent.as_ref() == Some(&item.id))
            .map(|it| it.id.clone())
            .collect();
        // 1-based ordinal among siblings in the same column (matches `pinto show`):
        // children rank against their parent's children, roots against roots.
        let rank_ordinal = board_items(&self.full_board)
            .filter(|it| {
                it.parent == item.parent && it.status == item.status && it.rank <= item.rank
            })
            .count();
        Some(PopupContent {
            id: item.id.clone(),
            title: item.title.clone(),
            status: item.status.clone(),
            acceptance_criteria: pinto::backlog::AcceptanceCriteriaProgress::from_markdown(
                &item.body,
            ),
            rank: item.rank.clone(),
            rank_ordinal,
            points: item.points,
            labels: item.labels.clone(),
            assignee: item.assignee.clone(),
            sprint: item.sprint.clone(),
            commits: item.commits.clone(),
            body: item.body.clone(),
            parent: item.parent.as_ref().map(ItemId::to_string),
            children,
            depends_on: item.depends_on.clone(),
            dependents: deps.summary(item).dependents,
            start_at: item.start_at,
            done_at: item.done_at,
            created: item.created,
            updated: item.updated,
        })
    }

    /// Open the Kanban help window and reset its scroll position.
    pub(crate) fn open_help(&mut self) {
        self.help_open = true;
        self.help_scroll = 0;
    }

    /// Close the Kanban help window. The selected board position is unchanged.
    pub(crate) fn close_help(&mut self) {
        self.help_open = false;
    }

    /// Is the Kanban help window open?
    pub(crate) fn is_help_open(&self) -> bool {
        self.help_open
    }

    /// Scroll position of the help window body.
    pub(crate) fn help_scroll(&self) -> u16 {
        self.help_scroll
    }

    /// Scroll the help window body by `delta` lines.
    pub(crate) fn scroll_help(&mut self, delta: i32, max: u16) {
        let current = i32::from(self.help_scroll);
        let next = (current + delta).clamp(0, i32::from(max));
        self.help_scroll = u16::try_from(next).unwrap_or(0);
    }

    /// Set the status (warning, etc.) to be temporarily displayed in the footer.
    pub(crate) fn set_status_message(&mut self, message: String) {
        self.status_message = Some(message);
    }

    /// Clears the temporary status of the footer (returns to key guide display).
    pub(crate) fn clear_status_message(&mut self) {
        self.status_message = None;
    }

    /// Current footer status (`None` if none exists).
    pub(crate) fn status_message(&self) -> Option<&str> {
        self.status_message.as_deref()
    }

    /// Columns rendered by this view, in workflow order.
    pub(crate) fn display_statuses(&self) -> &[String] {
        &self.display_statuses
    }

    /// Replace the display and full boards while keeping the expanded state.
    pub(crate) fn set_boards(&mut self, board: Board, full_board: Board) {
        self.board = board;
        self.full_board = full_board;
        self.clamp();
    }

    /// Replace the filter used by subsequent board reloads.
    pub(crate) fn set_search(&mut self, search: Option<SearchFilter>) {
        self.query.search = search;
    }

    /// Current Kanban search filter.
    pub(crate) fn search_filter(&self) -> Option<&SearchFilter> {
        self.query.search.as_ref()
    }

    /// Query used to load the current board, including startup and live-search filters.
    pub(crate) fn board_query(&self) -> &BoardQuery {
        &self.query
    }

    /// Open the vim-style search prompt in `mode` with an empty buffer, snapshotting the currently
    /// applied filter so a cancel can restore it.
    pub(crate) fn begin_search(&mut self, mode: SearchMode) {
        self.search_input = Some(SearchInput {
            mode,
            buffer: String::new(),
            error: None,
            restore: self.query.search.clone(),
        });
    }

    /// Is the vim-style search prompt open?
    pub(crate) fn is_searching(&self) -> bool {
        self.search_input.is_some()
    }

    /// Interpretation of the pending search input, if the prompt is open.
    pub(crate) fn search_input_mode(&self) -> Option<SearchMode> {
        self.search_input.as_ref().map(|input| input.mode)
    }

    /// Characters typed into the search prompt so far (empty string while closed).
    pub(crate) fn search_input_buffer(&self) -> &str {
        self.search_input
            .as_ref()
            .map_or("", |input| input.buffer.as_str())
    }

    /// Inline validation error under the prompt (`None` when the query is valid or the prompt is closed).
    pub(crate) fn search_input_error(&self) -> Option<&str> {
        self.search_input
            .as_ref()
            .and_then(|input| input.error.as_deref())
    }

    /// Append a typed character to the search prompt, clearing any stale validation error.
    pub(crate) fn push_search_char(&mut self, c: char) {
        if let Some(input) = self.search_input.as_mut() {
            input.buffer.push(c);
            input.error = None;
        }
    }

    /// Erase the last character from the search prompt, clearing any stale validation error.
    pub(crate) fn pop_search_char(&mut self) {
        if let Some(input) = self.search_input.as_mut() {
            input.buffer.pop();
            input.error = None;
        }
    }

    /// Record an inline validation error, keeping the prompt open so the query can be corrected.
    pub(crate) fn set_search_input_error(&mut self, message: String) {
        if let Some(input) = self.search_input.as_mut() {
            input.error = Some(message);
        }
    }

    /// Close the prompt after a successful commit, leaving the applied filter in place.
    pub(crate) fn end_search(&mut self) {
        self.search_input = None;
    }

    /// Close the prompt and yield the filter to restore (the one applied when it opened).
    ///
    /// The caller reloads the board through the returned filter so a cancelled incremental search
    /// rolls back to the pre-search view.
    pub(crate) fn take_search_restore(&mut self) -> Option<SearchFilter> {
        self.search_input.take().and_then(|input| input.restore)
    }

    /// Open the add form, which collects the title, body, parent, and dependencies in order.
    pub(crate) fn begin_add(&mut self) {
        self.form_input = Some(FormInput {
            mode: InputMode::AddTitle,
            buffer: String::new(),
            title: None,
            body: None,
            parent: None,
            depends_on: Vec::new(),
            source: None,
            selection_anchor: self.selected_item().map(|item| item.id.clone()),
            error: None,
        });
    }

    /// Open the dependency-add form for the selected item. Returns `false` when no item is selected.
    pub(crate) fn begin_dependency_add(&mut self) -> bool {
        self.begin_dependency(InputMode::DependencyAdd)
    }

    /// Open the dependency-remove form for the selected item. Returns `false` when no item is selected.
    pub(crate) fn begin_dependency_remove(&mut self) -> bool {
        self.begin_dependency(InputMode::DependencyRemove)
    }

    /// Open the parent form for the selected item. An empty submission clears its parent.
    pub(crate) fn begin_parent(&mut self) -> bool {
        self.begin_dependency(InputMode::Parent)
    }

    fn begin_dependency(&mut self, mode: InputMode) -> bool {
        let Some(source) = self.selected_item().map(|item| item.id.clone()) else {
            return false;
        };
        self.form_input = Some(FormInput {
            mode,
            buffer: String::new(),
            title: None,
            body: None,
            parent: None,
            depends_on: Vec::new(),
            source: Some(source),
            selection_anchor: None,
            error: None,
        });
        true
    }

    /// Whether an add/relation form is active.
    pub(crate) fn is_input_active(&self) -> bool {
        self.form_input.is_some()
    }

    /// Current form step.
    pub(crate) fn input_mode(&self) -> Option<InputMode> {
        self.form_input.as_ref().map(|input| input.mode)
    }

    /// Whether the active form can use the selected board item as its target.
    pub(crate) fn is_relation_input(&self) -> bool {
        matches!(
            self.input_mode(),
            Some(
                InputMode::AddParent
                    | InputMode::AddDependencies
                    | InputMode::DependencyAdd
                    | InputMode::DependencyRemove
                    | InputMode::Parent,
            )
        )
    }

    /// Characters typed in the active form.
    pub(crate) fn input_buffer(&self) -> &str {
        self.form_input
            .as_ref()
            .map_or("", |input| input.buffer.as_str())
    }

    /// Inline validation error shown under the active form.
    pub(crate) fn input_error(&self) -> Option<&str> {
        self.form_input
            .as_ref()
            .and_then(|input| input.error.as_deref())
    }

    /// Append a character and clear a stale form error.
    pub(crate) fn push_input_char(&mut self, c: char) {
        if let Some(input) = self.form_input.as_mut() {
            input.buffer.push(c);
            input.error = None;
        }
    }

    /// Erase the last character and clear a stale form error.
    pub(crate) fn pop_input_char(&mut self) {
        if let Some(input) = self.form_input.as_mut() {
            input.buffer.pop();
            input.error = None;
        }
    }

    /// Report a validation error without leaving the form.
    pub(crate) fn set_input_error(&mut self, message: String) {
        if let Some(input) = self.form_input.as_mut() {
            input.error = Some(message);
        }
    }

    /// Advance the form or return a persistence-ready submission.
    pub(crate) fn submit_input(&mut self) -> Result<InputSubmission, InputValidation> {
        let selected_target = self.selected_item().map(|item| item.id.clone());
        let Some(input) = self.form_input.as_mut() else {
            return Err(InputValidation::EmptyDependency);
        };
        match input.mode {
            InputMode::AddTitle => {
                if input.buffer.trim().is_empty() {
                    return Err(InputValidation::EmptyTitle);
                }
                let title = input.buffer.clone();
                input.mode = InputMode::AddBody;
                input.title = Some(title.clone());
                input.buffer.clear();
                Ok(InputSubmission::AddTitle { title })
            }
            InputMode::AddBody => {
                input.body = Some(input.buffer.clone());
                input.buffer.clear();
                input.mode = InputMode::AddParent;
                Ok(InputSubmission::AddStep)
            }
            InputMode::AddParent => {
                let raw_parent = input.buffer.trim().to_string();
                let cursor_target = selected_target
                    .clone()
                    .filter(|target| input.selection_anchor.as_ref() != Some(target));
                let parent = if raw_parent.is_empty() {
                    cursor_target
                } else {
                    Some(
                        raw_parent
                            .parse::<ItemId>()
                            .map_err(InputValidation::InvalidItemId)?,
                    )
                };
                input.parent = parent;
                input.selection_anchor = selected_target;
                input.buffer.clear();
                input.mode = InputMode::AddDependencies;
                Ok(InputSubmission::AddStep)
            }
            InputMode::AddDependencies => {
                let raw_dependencies = input.buffer.trim().to_string();
                let cursor_target = selected_target
                    .filter(|target| input.selection_anchor.as_ref() != Some(target));
                input.depends_on = if raw_dependencies.is_empty() {
                    cursor_target.into_iter().collect()
                } else {
                    raw_dependencies
                        .split_whitespace()
                        .map(|value| {
                            value
                                .parse::<ItemId>()
                                .map_err(InputValidation::InvalidItemId)
                        })
                        .collect::<Result<Vec<_>, _>>()?
                };
                Ok(InputSubmission::Add {
                    title: input.title.clone().unwrap_or_default(),
                    body: input.body.clone().unwrap_or_default(),
                    parent: input.parent.clone(),
                    depends_on: input.depends_on.clone(),
                })
            }
            InputMode::DependencyAdd | InputMode::DependencyRemove => {
                let dependency = if input.buffer.trim().is_empty() {
                    selected_target
                        .filter(|target| input.source.as_ref() != Some(target))
                        .map(|target| target.to_string())
                } else {
                    Some(input.buffer.trim().to_string())
                };
                let Some(dependency) = dependency else {
                    return Err(InputValidation::EmptyDependency);
                };
                let Some(source) = input.source.clone() else {
                    return Err(InputValidation::EmptyDependency);
                };
                Ok(InputSubmission::Dependency {
                    source,
                    dependency,
                    remove: input.mode == InputMode::DependencyRemove,
                })
            }
            InputMode::Parent => {
                let Some(source) = input.source.clone() else {
                    return Err(InputValidation::EmptyDependency);
                };
                let parent = if input.buffer.trim().is_empty() {
                    selected_target
                        .filter(|target| input.source.as_ref() != Some(target))
                        .map(|target| target.to_string())
                } else {
                    Some(input.buffer.trim().to_string())
                };
                Ok(InputSubmission::Parent { source, parent })
            }
        }
    }

    /// Close the active form, normally after cancellation or successful persistence.
    pub(crate) fn end_input(&mut self) {
        self.form_input = None;
    }

    /// Build a dependency index (reverse lookup of dependency source + completion set) from the entire board.
    pub(crate) fn dependency_index(&self) -> DependencyIndex {
        DependencyIndex::from_board(&self.full_board)
    }

    /// Visible display row for specified column (flattening parent-child tree to reflect folding).
    pub(crate) fn visible_rows(&self, col: usize) -> Vec<DisplayRow> {
        match self.board.columns.get(col) {
            Some(c) => column_display_rows(&c.items, &self.expanded),
            None => Vec::new(),
        }
    }

    /// If the selected visible row is a parent (has children), toggle expansion/collapse. If there are no children, ignore it.
    pub(crate) fn toggle_expand(&mut self) {
        let rows = self.visible_rows(self.col);
        let Some(dr) = rows.get(self.row) else {
            return;
        };
        if dr.child_count == 0 {
            return;
        }
        let id = self.board.columns[self.col].items[dr.item_index].id.clone();
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
        self.clamp();
    }

    /// Expand ancestors in the same column of `id` (to make collapsed children visible).
    fn expand_ancestors(&mut self, col: usize, id: &ItemId) {
        let to_expand = {
            let items = &self.board.columns[col].items;
            let in_col: HashSet<&ItemId> = items.iter().map(|it| &it.id).collect();
            let parent_of = |x: &ItemId| {
                items
                    .iter()
                    .find(|it| &it.id == x)
                    .and_then(|it| it.parent.clone())
            };
            let mut cur = parent_of(id);
            let mut acc = Vec::new();
            let mut seen: HashSet<ItemId> = HashSet::new();
            while let Some(pid) = cur {
                if !in_col.contains(&pid) || !seen.insert(pid.clone()) {
                    break;
                }
                acc.push(pid.clone());
                cur = parent_of(&pid);
            }
            acc
        };
        for pid in to_expand {
            self.expanded.insert(pid);
        }
    }

    /// Column being displayed.
    pub(crate) fn columns(&self) -> &[BoardColumn] {
        &self.board.columns
    }

    /// Selected column index.
    pub(crate) fn selected_col(&self) -> usize {
        self.col
    }

    /// Selected row index.
    pub(crate) fn selected_row(&self) -> usize {
        self.row
    }

    /// Column index displayed at the left end (horizontal scroll position).
    pub(crate) fn col_offset(&self) -> usize {
        self.col_offset
    }

    /// Return the selected PBI, or `None` when the visible row is empty.
    pub(crate) fn selected_item(&self) -> Option<&BacklogItem> {
        let rows = self.visible_rows(self.col);
        let dr = rows.get(self.row)?;
        self.board.columns.get(self.col)?.items.get(dr.item_index)
    }

    /// Clamp the selected position after columns or visible rows change.
    fn clamp(&mut self) {
        if self.board.columns.is_empty() {
            self.col = 0;
            self.row = 0;
            self.col_offset = 0;
            return;
        }
        if self.col >= self.board.columns.len() {
            self.col = self.board.columns.len() - 1;
        }
        let len = self.visible_rows(self.col).len();
        self.row = if len == 0 { 0 } else { self.row.min(len - 1) };
    }

    /// Move the selected column left and clamp the row to the destination column.
    pub(crate) fn select_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
            self.clamp();
            self.reset_popup_scroll_if_open();
        }
    }

    /// Move the selected column right and clamp the row to the destination column.
    pub(crate) fn select_right(&mut self) {
        if self.col + 1 < self.board.columns.len() {
            self.col += 1;
            self.clamp();
            self.reset_popup_scroll_if_open();
        }
    }

    /// Move selected row up one line.
    pub(crate) fn select_up(&mut self) {
        self.row = self.row.saturating_sub(1);
        self.reset_popup_scroll_if_open();
    }

    /// Move the selected row down one line, stopping at the last visible row.
    pub(crate) fn select_down(&mut self) {
        let len = self.visible_rows(self.col).len();
        if len > 0 && self.row + 1 < len {
            self.row += 1;
        }
        self.reset_popup_scroll_if_open();
    }

    /// When the details popup follows the selection, its body starts from the top for the new item.
    /// A no-op while the popup is closed, so plain board navigation is unaffected.
    fn reset_popup_scroll_if_open(&mut self) {
        if self.popup_open {
            self.popup_scroll = 0;
        }
    }

    /// Return the target for moving the selected PBI by `delta` (`-1` left, `+1` right).
    ///
    /// Return `None` at an edge or when the row is empty. The caller persists the move with
    /// [`pinto::service::move_item`] and calls [`Self::select_id`] after reloading.
    pub(crate) fn move_target(&self, delta: isize) -> Option<(ItemId, String)> {
        let item = self.selected_item()?;
        let current = self
            .display_statuses
            .iter()
            .position(|status| status == item.status.as_str())
            .and_then(|index| isize::try_from(index).ok())?;
        let target = usize::try_from(current.checked_add(delta)?).ok()?;
        let status = self.display_statuses.get(target)?;
        Some((item.id.clone(), status.clone()))
    }

    /// Return the target for reordering the selected PBI by `delta` (`-1` up, `+1` down) within
    /// its column.
    ///
    /// Reordering is scoped to **visual siblings** (rows with the same parent); descendant
    /// subtrees are skipped. Return `None` when there is no sibling in the requested direction.
    /// The caller changes only `rank` with [`pinto::service::reorder_item`].
    pub(crate) fn reorder_target(&self, delta: isize) -> Option<(ItemId, ReorderTarget)> {
        let rows = self.visible_rows(self.col);
        let cur = rows.get(self.row)?;
        let items = &self.board.columns.get(self.col)?.items;
        let moved = &items[cur.item_index];
        if delta < 0 {
            let sib = rows[..self.row]
                .iter()
                .rev()
                .find(|r| r.parent_index == cur.parent_index)?;
            Some((
                moved.id.clone(),
                ReorderTarget::Before(items[sib.item_index].id.clone()),
            ))
        } else if delta > 0 {
            let sib = rows[self.row + 1..]
                .iter()
                .find(|r| r.parent_index == cur.parent_index)?;
            Some((
                moved.id.clone(),
                ReorderTarget::After(items[sib.item_index].id.clone()),
            ))
        } else {
            None
        }
    }

    /// Adjust horizontal scrolling so the selected column fits in a `capacity`-column viewport.
    ///
    /// Scroll only when the selected column leaves the viewport, then clamp the offset so there is
    /// no trailing margin.
    pub(crate) fn scroll_to_visible(&mut self, capacity: usize) {
        let len = self.board.columns.len();
        if capacity == 0 || len == 0 {
            self.col_offset = 0;
            return;
        }
        if self.col < self.col_offset {
            self.col_offset = self.col;
        } else if self.col >= self.col_offset + capacity {
            self.col_offset = self.col + 1 - capacity;
        }
        let max_offset = len.saturating_sub(capacity);
        self.col_offset = self.col_offset.min(max_offset);
    }

    /// Select the PBI with `id`, expanding folded ancestors when necessary. Clamp the selection when
    /// the ID is not found.
    pub(crate) fn select_id(&mut self, id: &ItemId) {
        let found = self
            .board
            .columns
            .iter()
            .position(|c| c.items.iter().any(|it| &it.id == id));
        let Some(ci) = found else {
            self.clamp();
            return;
        };
        self.col = ci;
        self.expand_ancestors(ci, id);
        let rows = self.visible_rows(ci);
        let items = &self.board.columns[ci].items;
        if let Some(ri) = rows.iter().position(|r| &items[r.item_index].id == id) {
            self.row = ri;
        } else {
            self.clamp();
        }
    }
}
