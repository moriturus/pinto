//! Frame rendering for the Kanban view: columns, cards, footer, and popups.

use super::effective_capacity;
use crate::cli::kanban::keymap::KeyMap;
use crate::cli::kanban::{BoardView, InputMode, PopupContent, display_width, wrap};
use pinto::backlog::ItemId;
use pinto::i18n::{Message, current};
use pinto::kanban_keys::KeyAction;
use pinto::service::SearchMode;
use pinto::timezone::DisplayTimezone;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

/// Background color for a selected item; apply it to the entire row, including the ID/key cell.
const SELECTION_BG: Color = Color::Cyan;

/// The maximum scroll position that the body of the details view popup can take.
///
/// If the terminal size cannot be obtained, use a conservative default for the calculation.
/// The result is the number of wrapped content lines minus the number of visible popup lines.
pub(crate) fn popup_max_scroll(
    view: &BoardView,
    size: Option<ratatui::layout::Size>,
    keymap: &KeyMap,
) -> u16 {
    let Some(content) = view.popup_content() else {
        return 0;
    };
    let (width, height) = size.map_or((DEFAULT_POPUP_AREA.0, DEFAULT_POPUP_AREA.1), |s| {
        (s.width, s.height)
    });
    // `render` overlays the popup in the center area excluding the header and variable height footer.
    // Use the same base height as `render` so scrolling matches the drawn popup.
    let footer_height = footer_lines(view, width.saturating_sub(2), keymap).len() as u16;
    let board_area_height = height.saturating_sub(1 + footer_height);
    let area = popup_rect(Rect::new(0, 0, width, board_area_height));
    let inner_width = area.width.saturating_sub(2) as usize; // left and right frames.
    let inner_height = area.height.saturating_sub(2); // upper and lower frames.
    let total_lines = popup_lines_with_timezone(
        &content,
        inner_width,
        view.render_markdown(),
        view.display_timezone(),
    )
    .len() as u16;
    total_lines.saturating_sub(inner_height)
}

/// The maximum scroll position of the Kanban help window.
pub(crate) fn help_max_scroll(
    view: &BoardView,
    size: Option<ratatui::layout::Size>,
    keymap: &KeyMap,
) -> u16 {
    let (width, height) = size.map_or((DEFAULT_POPUP_AREA.0, DEFAULT_POPUP_AREA.1), |s| {
        (s.width, s.height)
    });
    // Keep this calculation in sync with `render`: the board area excludes the one-line header
    // and the variable-height prompt/status/footer area.
    let footer_height = footer_lines(view, width.saturating_sub(2), keymap).len() as u16;
    let board_area_height = height.saturating_sub(1 + footer_height);
    let show_clear_filter = view.search_filter().is_some();
    let area = help_popup_rect(
        Rect::new(0, 0, width, board_area_height),
        show_clear_filter,
        keymap,
    );
    let inner_height = area.height.saturating_sub(2);
    help_lines(keymap, show_clear_filter)
        .len()
        .try_into()
        .unwrap_or(u16::MAX)
        .saturating_sub(inner_height)
}

/// Default (width, height) used to calculate popup dimensions when device size is unknown.
const DEFAULT_POPUP_AREA: (u16, u16) = (80, 24);

pub(crate) fn render(frame: &mut Frame, view: &BoardView, confirming: bool, keymap: &KeyMap) {
    let footer_width = frame.area().width.saturating_sub(2);
    let footer_lines = footer_lines(view, footer_width, keymap);
    let footer_height = footer_lines
        .len()
        .min(frame.area().height.saturating_sub(1) as usize) as u16;
    let rows = Layout::vertical([
        Constraint::Length(1),             // header
        Constraint::Min(0),                // board
        Constraint::Length(footer_height), // Footer (key guide)
    ])
    .split(frame.area());
    let footer_area = footer_content_area(rows[2]);

    frame.render_widget(header(view, rows[0].width), rows[0]);
    render_columns(frame, view, rows[1]);
    // Footer: input/search prompts are shown directly (like Vim), any temporary status (for example, a WIP
    // warning) is highlighted, and otherwise the key guide is dimmed.
    let footer = Paragraph::new(Text::from(footer_lines)).style(
        if view.is_input_active() || view.is_searching() {
            Style::new()
        } else if view.status_message().is_some() {
            Style::new().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::new().add_modifier(Modifier::DIM)
        },
    );
    frame.render_widget(footer, footer_area);
    // Vim-style: park the terminal cursor at the end of the query on the bottom prompt line.
    if view.is_input_active() || view.is_searching() {
        let prompt_row = footer_area.bottom().saturating_sub(1);
        let prompt_width = view
            .input_mode()
            .map(input_prompt)
            .map_or(1, |prompt| display_width(&prompt) as u16 + 1);
        let cursor_x = footer_area
            .x
            .saturating_add(if view.is_input_active() {
                prompt_width
            } else {
                1
            })
            .saturating_add(display_width(if view.is_input_active() {
                view.input_buffer()
            } else {
                view.search_input_buffer()
            }) as u16)
            .min(footer_area.right().saturating_sub(1));
        frame.set_cursor_position(ratatui::layout::Position::new(cursor_x, prompt_row));
    }

    // While the details popup is open it always overlays the board — even on an empty column,
    // where a placeholder is shown so navigating there does not look like a return to normal mode.
    if view.is_popup_open() {
        let popup = popup_rect(rows[1]);
        match view.popup_content() {
            Some(content) => render_item_popup(
                frame,
                &content,
                view.popup_scroll(),
                view.render_markdown(),
                view.display_timezone(),
                rows[1],
            ),
            None => render_empty_popup(frame, rows[1]),
        }
        // Repair any full-width board glyph whose right half spills onto the popup's left border.
        sanitize_left_border(frame.buffer_mut(), popup);
    } else if confirming {
        render_quit_popup(frame, rows[1]);
    }

    // Help is a second-level overlay: it can be opened from either board or details mode while
    // the five primary operations remain visible in the fixed footer.
    if view.is_help_open() {
        render_help_popup(
            frame,
            view.help_scroll(),
            view.search_filter().is_some(),
            keymap,
            rows[1],
        );
    }
}

/// Inset the footer by one cell on both sides so its content aligns with the inside of column frames.
fn footer_content_area(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(2),
        area.height,
    )
}

/// Overlaying the details popup with [`Clear`] cannot fix a full-width (CJK) glyph sitting in the
/// board column immediately left of the popup's left border: the glyph's right half spills onto the
/// border cell and breaks the frame (ratatui reconciles wide glyphs only within a single widget, not
/// across an overlay boundary). Blank any such glyph so the border draws cleanly. Full-width glyphs
/// only ever spill rightward, so only the left border needs this treatment.
pub(super) fn sanitize_left_border(buf: &mut Buffer, popup: Rect) {
    if popup.x == 0 || popup.width == 0 {
        return;
    }
    let x = popup.x - 1;
    for y in popup.y..popup.y.saturating_add(popup.height) {
        if let Some(cell) = buf.cell_mut((x, y))
            && display_width(cell.symbol()) > 1
        {
            cell.set_symbol(" ");
        }
    }
}

/// Footer operation guide lines.
///
/// A temporary status (WIP warning, in-popup edit result, etc.) always wins the footer so the user
/// never misses it. Input prompts likewise own the footer while active. Otherwise the details
/// popup uses its own close/scroll/select/edit guide, while board mode shows the five primary
/// operations and keeps secondary operations in help.
pub(super) fn footer_lines(view: &BoardView, width: u16, keymap: &KeyMap) -> Vec<Line<'static>> {
    // The add/relation form and vim-style search prompt own the bottom line while open.
    if let Some(mode) = view.input_mode() {
        return input_prompt_lines(mode, view.input_buffer(), view.input_error());
    }
    if let Some(mode) = view.search_input_mode() {
        return search_prompt_lines(mode, view.search_input_buffer(), view.search_input_error());
    }
    if let Some(message) = view.status_message() {
        return vec![Line::from(format!(" {message} "))];
    }
    if view.is_popup_open() {
        return wrap_hint_groups(&popup_hints(keymap), width);
    }
    footer_hint_lines(keymap, width)
}

/// Build the fixed footer guide from the first configured key of the five primary operations.
fn key_hints(keymap: &KeyMap) -> String {
    let columns = key_pair(keymap, KeyAction::SelectLeft, KeyAction::SelectRight);
    let select = key_pair(keymap, KeyAction::SelectDown, KeyAction::SelectUp);
    let cursor = format!("{columns},{select}");
    current().format(
        Message::KanbanKeyHints,
        [
            ("cursor", cursor.as_str()),
            ("expand", keymap.first(KeyAction::ToggleExpand)),
            ("details", keymap.first(KeyAction::Details)),
            ("quit", keymap.first(KeyAction::Quit)),
        ],
    )
}

/// Build the footer lines with the help hint anchored to the right edge.
fn footer_hint_lines(keymap: &KeyMap, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    let help = current().format(
        Message::KanbanHelpHint,
        [("help", keymap.first(KeyAction::Help))],
    );
    let help_width = display_width(&help);
    let mut lines = wrap_hint_groups(&key_hints(keymap), width as u16);
    let Some(last) = lines.last_mut() else {
        return vec![right_aligned_hint(&help, width)];
    };
    let last_text = last.to_string();
    let last_width = display_width(&last_text);
    if last_text.is_empty() {
        *last = right_aligned_hint(&help, width);
    } else if last_width.saturating_add(2).saturating_add(help_width) <= width {
        let mut combined = last_text;
        combined.push_str(&" ".repeat(width - last_width - help_width));
        combined.push_str(&help);
        *last = Line::from(combined);
    } else {
        lines.push(right_aligned_hint(&help, width));
    }
    lines
}

/// Pad one footer hint so its right edge reaches the requested display width.
fn right_aligned_hint(hint: &str, width: usize) -> Line<'static> {
    Line::from(format!(
        "{}{}",
        " ".repeat(width.saturating_sub(display_width(hint))),
        hint
    ))
}

/// Join two related key labels while preserving the keymap's configured spelling.
fn key_pair(keymap: &KeyMap, first: KeyAction, second: KeyAction) -> String {
    let first = keymap.first(first);
    let separator = if first.ends_with('/') { "" } else { "/" };
    format!("{first}{separator}{}", keymap.first(second))
}

/// Build the details-popup key guide from the first configured key of every popup operation.
fn popup_hints(keymap: &KeyMap) -> String {
    current().format(
        Message::KanbanPopupHints,
        [
            ("close", keymap.first(KeyAction::PopupClose)),
            ("scroll_up", keymap.first(KeyAction::PopupScrollUp)),
            ("scroll_down", keymap.first(KeyAction::PopupScrollDown)),
            ("select_up", keymap.first(KeyAction::PopupSelectUp)),
            ("select_down", keymap.first(KeyAction::PopupSelectDown)),
            ("select_left", keymap.first(KeyAction::PopupSelectLeft)),
            ("select_right", keymap.first(KeyAction::PopupSelectRight)),
            ("edit", keymap.first(KeyAction::Edit)),
        ],
    )
}

/// Build a single help entry key.
fn help_key(keymap: &KeyMap, action: KeyAction) -> String {
    keymap.first(action).to_string()
}

/// Build the help window entries from every accepted operation outside the fixed footer guide.
fn help_lines(keymap: &KeyMap, show_clear_filter: bool) -> Vec<Line<'static>> {
    let shell = help_key(keymap, KeyAction::Shell);
    let move_keys = key_pair(keymap, KeyAction::MoveLeft, KeyAction::MoveRight);
    let reorder = key_pair(keymap, KeyAction::ReorderUp, KeyAction::ReorderDown);
    let add = help_key(keymap, KeyAction::Add);
    let parent = help_key(keymap, KeyAction::Parent);
    let dependency_add = help_key(keymap, KeyAction::DependencyAdd);
    let dependency_remove = help_key(keymap, KeyAction::DependencyRemove);
    let edit = help_key(keymap, KeyAction::Edit);
    let reload = help_key(keymap, KeyAction::Reload);
    let maximize = help_key(keymap, KeyAction::Maximize);
    let search = help_key(keymap, KeyAction::Search);
    let regex_search = help_key(keymap, KeyAction::RegexSearch);
    let clear_filter = show_clear_filter.then(|| help_key(keymap, KeyAction::ClearFilter));
    let keys = [
        &shell,
        &move_keys,
        &reorder,
        &add,
        &parent,
        &dependency_add,
        &dependency_remove,
        &edit,
        &reload,
        &maximize,
        &search,
        &regex_search,
    ];
    let key_width = keys
        .iter()
        .map(|key| display_width(key))
        .chain(clear_filter.iter().map(|key| display_width(key)))
        .max()
        .unwrap_or(1);
    let pad = |key: &str| -> String {
        format!(
            "{key}{}",
            " ".repeat(key_width.saturating_sub(display_width(key)))
        )
    };
    let shell = pad(&shell);
    let move_keys = pad(&move_keys);
    let reorder = pad(&reorder);
    let add = pad(&add);
    let parent = pad(&parent);
    let dependency_add = pad(&dependency_add);
    let dependency_remove = pad(&dependency_remove);
    let edit = pad(&edit);
    let reload = pad(&reload);
    let maximize = pad(&maximize);
    let search = pad(&search);
    let regex_search = pad(&regex_search);
    let clear_filter = clear_filter.map(|key| pad(&key));
    let entries = current().format(
        Message::KanbanHelpEntries,
        [
            ("shell", shell.as_str()),
            ("move", move_keys.as_str()),
            ("reorder", reorder.as_str()),
            ("add", add.as_str()),
            ("parent", parent.as_str()),
            ("dependency_add", dependency_add.as_str()),
            ("dependency_remove", dependency_remove.as_str()),
            ("edit", edit.as_str()),
            ("reload", reload.as_str()),
            ("maximize", maximize.as_str()),
            ("search", search.as_str()),
            ("regex_search", regex_search.as_str()),
        ],
    );
    let mut lines: Vec<Line<'static>> = entries
        .lines()
        .map(|line| Line::from(line.to_string()))
        .collect();
    if let Some(clear_filter) = clear_filter {
        let clear_filter = current().format(
            Message::KanbanHelpClearFilter,
            [("clear_filter", clear_filter.as_str())],
        );
        lines.push(Line::from(clear_filter));
    }
    lines
}

/// Build the vim-style search prompt: a `/` (substring) or `?` (regex) prefix plus the typed query.
///
/// The prompt itself is always the last line so the terminal cursor can sit at the bottom edge like
/// vim; a validation error, when present, is shown on the line just above it.
fn search_prompt_lines(mode: SearchMode, buffer: &str, error: Option<&str>) -> Vec<Line<'static>> {
    let prefix = match mode {
        SearchMode::Contains => '/',
        SearchMode::Regex => '?',
    };
    let mut lines = Vec::new();
    if let Some(error) = error {
        lines.push(Line::from(format!(" {error} ")));
    }
    lines.push(Line::from(format!("{prefix}{buffer}")));
    lines
}

/// Build the add/relation prompt and optional inline validation line.
fn input_prompt_lines(mode: InputMode, buffer: &str, error: Option<&str>) -> Vec<Line<'static>> {
    let prompt = input_prompt(mode);
    let mut lines = Vec::new();
    if let Some(error) = error {
        lines.push(Line::from(format!(" {error} ")));
    }
    lines.push(Line::from(format!("{prompt} {buffer}")));
    lines
}

/// Localized label for an add/relation prompt, without the input separator.
fn input_prompt(mode: InputMode) -> String {
    match mode {
        InputMode::AddTitle => current().text(Message::KanbanAddTitlePrompt),
        InputMode::AddBody => current().text(Message::KanbanAddBodyPrompt),
        InputMode::AddParent => current().text(Message::KanbanAddParentPrompt),
        InputMode::AddDependencies => current().text(Message::KanbanAddDependenciesPrompt),
        InputMode::DependencyAdd => current().text(Message::KanbanDependencyAddPrompt),
        InputMode::DependencyRemove => current().text(Message::KanbanDependencyRemovePrompt),
        InputMode::Parent => current().text(Message::KanbanParentPrompt),
    }
}

/// Wrap a `  `-separated key-hint string into footer lines in units of operations.
///
/// Do not separate key-action pairs like `h/l: columns` and only when one pair exceeds the screen width.
/// Defer to normal word boundary wrapping.
fn wrap_hint_groups(hints: &str, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    let mut lines = Vec::<String>::new();
    let mut line = String::new();
    for group in hints.split("  ").filter(|group| !group.is_empty()) {
        let group_width = display_width(group);
        let separator = usize::from(!line.is_empty()) * 2;
        if group_width <= width && display_width(&line) + separator + group_width <= width {
            if !line.is_empty() {
                line.push_str("  ");
            }
            line.push_str(group);
        } else {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            if group_width <= width {
                line.push_str(group);
            } else {
                let mut wrapped = wrap(group, width);
                line = wrapped.pop().unwrap_or_default();
                lines.extend(wrapped);
            }
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines.into_iter().map(Line::from).collect()
}

/// header row. Title, visibility range/direction indicator during horizontal scrolling, and legend for dependent markers.
pub(crate) fn header(view: &BoardView, width: u16) -> Line<'static> {
    let total = view.columns().len();
    let capacity = effective_capacity(width, view.is_maximized());
    let start = view.col_offset();
    let end = (start + capacity).min(total);
    let mut label = String::from(" pinto — kanban ");
    if total > capacity {
        let left = if start > 0 { "◀" } else { " " };
        let right = if end < total { "▶" } else { " " };
        let start = (start + 1).to_string();
        let end = end.to_string();
        let total = total.to_string();
        label.push_str(&format!(
            " {} ",
            current().format(
                Message::KanbanColumnRange,
                [
                    ("left", left),
                    ("start", start.as_str()),
                    ("end", end.as_str()),
                    ("total", total.as_str()),
                    ("right", right),
                ],
            )
        ));
    }
    let mut spans = vec![Span::styled(
        label,
        Style::new().add_modifier(Modifier::BOLD),
    )];
    // Surface an active search filter so items hidden by it read as filtered, not missing. While the
    // prompt is open the bottom line already echoes the query, so the header stays uncluttered.
    if let Some(filter) = view.search_filter().filter(|_| !view.is_searching()) {
        let message = match filter.mode() {
            SearchMode::Contains => Message::KanbanActiveFilter,
            SearchMode::Regex => Message::KanbanActiveRegexFilter,
        };
        let indicator = current().format(message, [("pattern", filter.pattern())]);
        spans.push(Span::styled(
            format!(" {indicator} "),
            Style::new().fg(Color::Black).bg(Color::Yellow),
        ));
    }
    // Dependency marker legend (string shared with board). Display more modestly than the main text.
    spans.push(Span::styled(
        format!(" {} ", crate::cli::kanban::dependency_legend(current())),
        Style::new().fg(Color::DarkGray),
    ));
    Line::from(spans)
}

/// Draw columns horizontally in the board area (fixed width/horizontal scrolling).
///
/// Highlight selected columns with a frame and selected rows with a background color. Each card
/// starts with its ID in the first row; the title is wrapped to the column width.
fn render_columns(frame: &mut Frame, view: &BoardView, area: Rect) {
    let columns = view.columns();
    if columns.is_empty() {
        frame.render_widget(
            Paragraph::new(current().text(Message::KanbanEmptyColumns)),
            area,
        );
        return;
    }
    let capacity = effective_capacity(area.width, view.is_maximized());
    let start = view.col_offset();
    let end = (start + capacity).min(columns.len());
    let visible = end - start;
    // Reverse lookup of dependent sources and completion determination require the entire board, so they are constructed only once in one frame.
    let deps = view.dependency_index();

    // Divide the drawing area equally into visible columns and fill it (if there is room, it will be wider than the minimum width).
    let constraints: Vec<Constraint> = (0..visible).map(|_| Constraint::Fill(1)).collect();
    let cells = Layout::horizontal(constraints).split(area);

    for (slot, ci) in (start..end).enumerate() {
        let cell = cells[slot];
        let column = &columns[ci];
        let selected_here = ci == view.selected_col();
        let inner_width = cell.width.saturating_sub(2) as usize; // Actual inside width excluding frame.
        let id_width = column
            .items
            .iter()
            .map(|item| display_width(&item.id.to_string()))
            .max()
            .unwrap_or(0);
        let items: Vec<ListItem> = view
            .visible_rows(ci)
            .iter()
            .map(|dr| {
                let it = &column.items[dr.item_index];
                let marker = fold_marker(dr);
                ListItem::new(card_lines(
                    &it.id.to_string(),
                    &it.title,
                    dr.depth,
                    marker,
                    id_width,
                    it.points,
                    it.assignee.as_deref(),
                    &deps.summary(it),
                    inner_width,
                ))
            })
            .collect();
        let title = format!(" {} ({}) ", column.status, column.items.len());
        let border = if selected_here {
            Style::new().fg(Color::Cyan)
        } else {
            Style::new()
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border)
            .title(title);
        let mut state = ListState::default();
        if selected_here && !column.items.is_empty() {
            state.select(Some(view.selected_row()));
        }
        // For selected lines, specify the background color and apply it uniformly to the entire line. ID span's own fg is
        // It will be overwritten (patch) here, and the key will also have the same background color as the main text (if it is reversed, the background will be
        // (varies).
        let list = List::new(items).block(block).highlight_style(
            Style::new()
                .bg(SELECTION_BG)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );
        frame.render_stateful_widget(list, cell, &mut state);
    }
}

/// Fixed-width child indicator rendered after each card ID.
const CHILD_INDICATOR_WIDTH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Marker {
    /// Has no children (the indicator slot remains reserved).
    None,
    /// Collapsed (`▸` + number of children).
    Collapsed(usize),
    /// Expanded (`▾`).
    Expanded,
}

impl Marker {
    /// Indicator display string, right-aligned in four cells (`▸99+` is the maximum).
    fn label(self) -> String {
        let indicator = match self {
            Marker::None => String::new(),
            Marker::Collapsed(n) => {
                let count = if n >= 100 {
                    "99+".to_string()
                } else {
                    n.to_string()
                };
                format!("▸{count}")
            }
            Marker::Expanded => "▾".to_string(),
        };
        let padding = CHILD_INDICATOR_WIDTH.saturating_sub(display_width(&indicator));
        format!("{}{}", " ".repeat(padding), indicator)
    }
}

/// Determine the fold marker from the displayed line.
pub(super) fn fold_marker(dr: &crate::cli::kanban::DisplayRow) -> Marker {
    if dr.child_count == 0 {
        Marker::None
    } else if dr.expanded {
        Marker::Expanded
    } else {
        Marker::Collapsed(dr.child_count)
    }
}

/// Format a single card into multiple lines. Indent to the depth and place a fixed-width ID and
/// child indicator before the title. The title is wrapped at the remaining width, and continuation
/// lines are aligned to the title column.
/// Story points (◆) and assignee (@) are appended as a muted meta line when set, followed by
/// a dependent (⊸)/dependent source (⊷) line if there is a dependency relationship.
#[allow(clippy::too_many_arguments)]
fn card_lines(
    id: &str,
    title: &str,
    depth: usize,
    marker: Marker,
    id_width: usize,
    points: Option<u32>,
    assignee: Option<&str>,
    deps: &crate::cli::kanban::DepSummary,
    inner_width: usize,
) -> Text<'static> {
    let indent = "  ".repeat(depth); // 1 row = 2 digits.
    let marker_label = marker.label();
    // Display width of "Indent + fixed-width ID + indicator + title separators". Continuation
    // lines are indented by this width to align the title column.
    let prefix_width = display_width(&indent) + id_width + display_width(&marker_label) + 2;
    let title_width = inner_width.saturating_sub(prefix_width).max(1);
    let segments = wrap(title, title_width);
    let id_style = Style::new()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::BOLD);
    let marker_style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let cont_indent = " ".repeat(prefix_width);
    let mut lines: Vec<Line> = Vec::with_capacity(segments.len().max(1) + 1);
    for (i, seg) in segments.iter().enumerate() {
        if i == 0 {
            let mut spans = Vec::with_capacity(4);
            if !indent.is_empty() {
                spans.push(Span::raw(indent.clone()));
            }
            let id_padding = id_width.saturating_sub(display_width(id));
            spans.push(Span::styled(
                format!("{id}{}", " ".repeat(id_padding)),
                id_style,
            ));
            spans.push(Span::raw(" "));
            if marker == Marker::None {
                spans.push(Span::raw(marker_label.clone()));
            } else {
                spans.push(Span::styled(marker_label.clone(), marker_style));
            }
            spans.push(Span::raw(" "));
            spans.push(Span::raw(seg.clone()));
            lines.push(Line::from(spans));
        } else {
            lines.push(Line::from(format!("{cont_indent}{seg}")));
        }
    }
    if let Some(line) = meta_line(points, assignee, &cont_indent) {
        lines.push(line);
    }
    if let Some(line) = dependency_line(deps, &cont_indent) {
        lines.push(line);
    }
    Text::from(lines)
}

/// Story points/assignee summary line (`◆ 5  @alice`). `None` when neither is set.
///
/// Indented to align with the title column like the dependency line, and drawn in a muted color so
/// it reads as metadata rather than the card's main text. Either field is shown independently, so an
/// unestimated but assigned card (or vice versa) still gets a meta line.
fn meta_line(
    points: Option<u32>,
    assignee: Option<&str>,
    cont_indent: &str,
) -> Option<Line<'static>> {
    if points.is_none() && assignee.is_none() {
        return None;
    }
    let style = Style::new().fg(Color::DarkGray);
    let mut spans: Vec<Span<'static>> = vec![Span::raw(cont_indent.to_string())];
    if let Some(points) = points {
        spans.push(Span::styled(format!("◆ {points}"), style));
    }
    if let Some(assignee) = assignee {
        if spans.len() > 1 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(format!("@{assignee}"), style));
    }
    Some(Line::from(spans))
}

/// Dependency line (`⊸ Depends on` `⊷ Depends on`). `None` if there is no dependency.
///
/// Indent and align to title column. If unfinished dependencies remain (blocked), mark them.
/// Draw it in red as `⊸!` so that it can be identified even in environments where colors cannot be used (same symbol as board).
fn dependency_line(
    deps: &crate::cli::kanban::DepSummary,
    cont_indent: &str,
) -> Option<Line<'static>> {
    if deps.is_empty() {
        return None;
    }
    let mut spans: Vec<Span<'static>> = vec![Span::raw(cont_indent.to_string())];
    if !deps.depends_on.is_empty() {
        // Blocked is `⊸!` + red, resolved (all dependencies are completed) is `⊸` + calm color.
        let (mark, style) = if deps.blocked {
            (
                "⊸!",
                Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            )
        } else {
            ("⊸", Style::new().fg(Color::DarkGray))
        };
        spans.push(Span::styled(
            format!(
                "{mark} {}",
                crate::cli::kanban::format_ids(&deps.depends_on, crate::cli::kanban::DEP_ID_LIMIT)
            ),
            style,
        ));
    }
    if !deps.dependents.is_empty() {
        if spans.len() > 1 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            format!(
                "⊷ {}",
                crate::cli::kanban::format_ids(&deps.dependents, crate::cli::kanban::DEP_ID_LIMIT)
            ),
            Style::new().fg(Color::DarkGray),
        ));
    }
    Some(Line::from(spans))
}

/// Draw the completion confirmation popup overlapping the center. Make it the smallest size that fits the text.
fn render_quit_popup(frame: &mut Frame, area: Rect) {
    let body_text = current().text(Message::KanbanQuitBody);
    // Minimum width of 2 digits for frame + 2 digits for left and right padding. Match the title to the wider of the body.
    let title = format!(" {} ", current().text(Message::KanbanQuitPrompt));
    let content_width = display_width(&body_text).max(display_width(&title)) as u16;
    let popup = centered_fixed(content_width + 4, 3, area); // 1 line of text + top and bottom frames.
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Yellow))
        .title(title);
    let body = Paragraph::new(body_text)
        .block(block)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(body, popup);
}

/// Rectangle for the secondary-operation help window.
fn help_popup_rect(area: Rect, show_clear_filter: bool, keymap: &KeyMap) -> Rect {
    let lines = help_lines(keymap, show_clear_filter);
    let content_width = lines
        .iter()
        .map(|line| display_width(&line.to_string()))
        .max()
        .unwrap_or(1);
    let width = u16::try_from(content_width.saturating_add(4)).unwrap_or(u16::MAX);
    let height = u16::try_from(lines.len().saturating_add(2)).unwrap_or(u16::MAX);
    let width = width.max(1).min(area.width);
    let height = height.max(1).min(area.height);
    Rect {
        x: area.right().saturating_sub(width),
        y: area.bottom().saturating_sub(height),
        width,
        height,
    }
}

/// Draw the secondary-operation help window in the style of the details popup.
fn render_help_popup(
    frame: &mut Frame,
    scroll: u16,
    show_clear_filter: bool,
    keymap: &KeyMap,
    area: Rect,
) {
    let popup = help_popup_rect(area, show_clear_filter, keymap);
    frame.render_widget(Clear, popup);
    let title = format!(" {} ", current().text(Message::KanbanHelpTitle));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Cyan))
        .title(title);
    let body = Paragraph::new(Text::from(help_lines(keymap, show_clear_filter)))
        .block(block)
        .scroll((scroll, 0));
    frame.render_widget(body, popup);
    sanitize_left_border(frame.buffer_mut(), popup);
}

/// Place a rectangle of the specified size (not exceeding the area) in the center of the `area`.
fn centered_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// Return the details-popup rectangle, targeting 80% of `area`'s width and 90% of its height.
/// Clamp both dimensions so the rectangle never exceeds `area`, even on a very small terminal.
pub(super) fn popup_rect(area: Rect) -> Rect {
    let width = ((u32::from(area.width) * 4 / 5) as u16).max(1);
    let height = ((u32::from(area.height) * 9 / 10) as u16).max(1);
    centered_fixed(width, height, area)
}

/// Build the details-popup lines from the header, body, and relationship information.
///
/// `width` is the inner display width excluding the frame. The same width is used to calculate
/// scrolling, so this remains a pure function independent of drawing.
pub(super) fn popup_lines_with_timezone(
    content: &PopupContent,
    width: usize,
    markdown: bool,
    timezone: DisplayTimezone,
) -> Vec<Line<'static>> {
    fn field(label: &str, value: String) -> Line<'static> {
        Line::from(vec![
            Span::styled(
                format!("{label}: "),
                Style::new().add_modifier(Modifier::BOLD),
            ),
            Span::raw(value),
        ])
    }
    fn or_dash(value: Option<&str>) -> String {
        value.map(str::to_string).unwrap_or_else(|| "-".to_string())
    }
    // Match `pinto show`: use `-` for an empty list and `+N` only when many IDs are present.
    fn ids_or_dash(ids: &[ItemId]) -> String {
        if ids.is_empty() {
            "-".to_string()
        } else {
            crate::cli::kanban::format_ids(ids, 8)
        }
    }

    // Render an RFC3339 timestamp, or `-` when unset (matches `pinto show`).
    fn time_or_dash(
        value: Option<chrono::DateTime<chrono::Utc>>,
        timezone: DisplayTimezone,
    ) -> String {
        value
            .map(|d| timezone.format_datetime(d, "%Y-%m-%dT%H:%M:%S%:z"))
            .unwrap_or_else(|| "-".to_string())
    }

    let mut lines = vec![
        field("ID", content.id.to_string()),
        field("Title", content.title.clone()),
        field("Status", content.status.to_string()),
        field(
            "Acceptance Criteria",
            content.acceptance_criteria.to_string(),
        ),
    ];
    // Rank shows the sibling-local ordinal with the internal fractional index
    // (as in `pinto show`); a child names its parent so the number is clearly
    // the order among that parent's children, not the whole column.
    let rank_value = match &content.parent {
        Some(parent) => format!(
            "#{} under {} ({})",
            content.rank_ordinal, parent, content.rank
        ),
        None => format!("#{} ({})", content.rank_ordinal, content.rank),
    };
    lines.push(field("Rank", rank_value));
    lines.push(field(
        "Points",
        content
            .points
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string()),
    ));
    lines.push(field(
        "Labels",
        if content.labels.is_empty() {
            "-".to_string()
        } else {
            content.labels.join(", ")
        },
    ));
    lines.push(field("Assignee", or_dash(content.assignee.as_deref())));
    lines.push(field("Sprint", or_dash(content.sprint.as_deref())));
    lines.push(field("Parent", or_dash(content.parent.as_deref())));
    lines.push(field("Children", ids_or_dash(&content.children)));
    lines.push(field("Depends on", ids_or_dash(&content.depends_on)));
    lines.push(field("Depended by", ids_or_dash(&content.dependents)));
    lines.push(field("Started", time_or_dash(content.start_at, timezone)));
    lines.push(field("Completed", time_or_dash(content.done_at, timezone)));
    lines.push(field(
        "Commits",
        if content.commits.is_empty() {
            "-".to_string()
        } else {
            content.commits.join(", ")
        },
    ));
    lines.push(field(
        "Created",
        timezone.format_datetime(content.created, "%Y-%m-%dT%H:%M:%S%:z"),
    ));
    lines.push(field(
        "Updated",
        timezone.format_datetime(content.updated, "%Y-%m-%dT%H:%M:%S%:z"),
    ));
    lines.push(Line::default());

    if content.body.is_empty() {
        lines.push(Line::from(Span::styled(
            current().text(Message::KanbanNoBody),
            Style::new().fg(Color::DarkGray),
        )));
    } else if markdown {
        // Render the body as Markdown, sharing `pinto show`'s rendering path.
        lines.extend(crate::cli::markdown::render_lines(&content.body, width));
    } else {
        // Opt-out: wrap the raw Markdown text line by line (previous behaviour).
        for src_line in content.body.lines() {
            if src_line.is_empty() {
                lines.push(Line::default());
            } else {
                for wrapped in wrap(src_line, width) {
                    lines.push(Line::from(wrapped));
                }
            }
        }
    }
    lines
}

/// Draws the details viewing popup centered. `scroll` is the vertical scroll position of the text.
///
/// Since the background is erased with [`Clear`] before drawing, the characters will not be garbled even if they overlap with cards, etc.
/// If the terminal is small, [`popup_rect`] will be clamped to a width and height that does not exceed the area.
fn render_item_popup(
    frame: &mut Frame,
    content: &PopupContent,
    scroll: u16,
    markdown: bool,
    timezone: DisplayTimezone,
    area: Rect,
) {
    let popup = popup_rect(area);
    frame.render_widget(Clear, popup);
    let title = format!(
        " {} — {} ",
        content.id,
        current().text(Message::KanbanDetailsTitle),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Cyan))
        .title(title);
    let inner_width = popup.width.saturating_sub(2) as usize;
    let lines = popup_lines_with_timezone(content, inner_width, markdown, timezone);
    let body = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(body, popup);
}

/// Draws the details popup with a "no item selected" placeholder, used when the popup is open but
/// the selection is empty (e.g. after navigating to a column with no cards). Keeps the popup frame
/// on screen so the detail mode stays visible, and the title keeps advertising the close key.
fn render_empty_popup(frame: &mut Frame, area: Rect) {
    let popup = popup_rect(area);
    frame.render_widget(Clear, popup);
    let title = format!(" {} ", current().text(Message::KanbanDetailsTitle));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Cyan))
        .title(title);
    let body = Paragraph::new(current().text(Message::KanbanNoSelection))
        .block(block)
        .style(Style::new().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(body, popup);
}
