use super::board;
use super::board::format_board;
use super::item::{DetailOptions, ListLongOptions, format_detail, format_list_long};
use super::report::{format_burndown, format_cycletime, format_duration};
use super::sprint::format_sprints_with_timezone;
use super::text::{MIN_TITLE_WIDTH, display_width, truncate};
use crate::cli::dependency_display::dependency_legend;
use chrono::{DateTime, Duration, Utc};
use pinto::backlog::{BacklogItem, ItemId, Status};
use pinto::i18n::current;
use pinto::rank::Rank;
use pinto::service::{Board, BoardColumn, Burndown, CycleTimeReport, DurationSummary, ItemDetail};
use pinto::timezone::DisplayTimezone;
use unicode_width::UnicodeWidthStr;

#[test]
fn output_type_modules_expose_their_formatters() {
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items: vec![],
        }],
        orphaned: vec![],
    };

    assert_eq!(board::format_board(&board, 80), format_board(&board, 80));
}

fn item(number: u32, title: &str, status: &str) -> BacklogItem {
    let now = DateTime::<Utc>::from_timestamp(0, 0).expect("valid epoch");
    BacklogItem::new(
        ItemId::try_new("T", number).expect("safe test ID"),
        title,
        Status::new(status),
        Rank::between(None, None).expect("open bounds produce a rank"),
        now,
    )
    .expect("valid item")
}

// --- Detailed list (`list -l`) ---

#[test]
fn format_list_long_shows_status_points_labels_sprint_and_timestamps() {
    let mut it = item(1, "Detailed", "todo");
    it.points = Some(5);
    it.labels = vec!["backend".to_string(), "urgent".to_string()];
    it.sprint = Some("S-1".to_string());

    let out = format_list_long(
        std::slice::from_ref(&it),
        120,
        ListLongOptions::new(true, true),
    );

    assert!(out.contains("T-1"), "shows id: {out}");
    assert!(out.contains("todo"), "shows status: {out}");
    assert!(out.contains("Detailed"), "shows title: {out}");
    assert!(out.contains('5'), "shows points: {out}");
    assert!(
        out.contains("backend") && out.contains("urgent"),
        "shows labels: {out}"
    );
    assert!(out.contains("S-1"), "shows sprint: {out}");
    // The creation/update date and time (in days/UTC) will be displayed. Test item() uses epoch (1970-01-01).
    assert!(out.contains("1970-01-01"), "shows created/updated: {out}");
}

#[test]
fn format_list_long_shows_dash_for_unset_optional_fields() {
    let out = format_list_long(&[item(1, "Bare", "todo")], 120, ListLongOptions::default());
    assert!(out.contains('-'), "unset fields show dash: {out}");
}

#[test]
fn format_list_long_uses_configured_timezone_for_date_columns() {
    let mut it = item(1, "Boundary", "todo");
    let instant = DateTime::<Utc>::from_timestamp(30 * 60, 0).expect("timestamp");
    it.created = instant;
    it.updated = instant;
    let timezone = "-01:00".parse::<DisplayTimezone>().expect("offset");

    let out = format_list_long(
        &[it],
        120,
        ListLongOptions::default().with_timezone(timezone),
    );

    assert_eq!(
        out.lines()
            .nth(1)
            .expect("data row")
            .matches("1969-12-31")
            .count(),
        2,
        "both dates use the configured offset: {out}"
    );
}

#[test]
fn format_list_long_has_header_row() {
    let out = format_list_long(&[item(1, "Task", "todo")], 120, ListLongOptions::default());
    let first_line = out.lines().next().unwrap_or_default();
    // The header indicates the column name of meta information (ID / STATUS, etc., distinguished from other lines by uppercase letters).
    assert!(first_line.contains("ID"), "header row: {first_line}");
    assert!(first_line.contains("STATUS"), "header row: {first_line}");
    assert!(first_line.contains("TITLE"), "header row: {first_line}");
}

#[test]
fn format_list_long_is_empty_for_no_items() {
    assert_eq!(format_list_long(&[], 120, ListLongOptions::default()), "");
}

#[test]
fn format_list_long_respects_terminal_width() {
    let long = "x".repeat(300);
    let out = format_list_long(&[item(1, &long, "todo")], 60, ListLongOptions::default());
    for line in out.lines() {
        assert!(
            display_width(line) <= 60,
            "line exceeds width 60 ({}): {line:?}",
            display_width(line)
        );
    }
    assert!(out.contains('…'), "long title is truncated: {out}");
}

#[test]
fn format_list_long_unbounded_width_never_truncates() {
    let long = "x".repeat(300);
    let out = format_list_long(
        &[item(1, &long, "todo")],
        usize::MAX,
        ListLongOptions::default(),
    );
    assert!(out.contains(&long), "shows full title: {out}");
    assert!(!out.contains('…'), "no ellipsis: {out}");
}

#[test]
fn format_list_long_aligns_columns_with_fullwidth_titles() {
    // Even if full-width titles are mixed, the starting positions on the display width of subsequent columns (values such as POINTS) will be aligned.
    // With padding based on the number of characters (`{:<w$}`), there is a shift because 1 full-width character = 2 display widths.
    let mut a = item(1, "あいう", "todo");
    a.points = Some(5);
    let mut b = item(2, "short", "in-progress");
    b.points = Some(7);
    let out = format_list_long(&[a, b], 120, ListLongOptions::default());
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "header + 2 rows: {out}");

    // Each data row has equal display width before the POINTS value ('5' / '7', which does not appear in other column values).
    let points_col = |line: &str, needle: char| -> usize {
        let idx = line.find(needle).expect("points value present");
        display_width(&line[..idx])
    };
    let a_pos = points_col(lines[1], '5');
    let b_pos = points_col(lines[2], '7');
    assert_eq!(
        a_pos, b_pos,
        "POINTS column starts at same display column: \n{out}"
    );
}

#[test]
fn format_list_long_fits_width_80_with_long_status_and_labels() {
    // Fallback width 80 for non-TTY is the main case. Long states (in-progress) and long labels
    // Even if there is, all 8 column headers and date values will fit without relying on hard truncation at the end of the line.
    let mut it = item(1, "カンバン: 選択アイテムの詳細ポップアップ", "in-progress");
    it.labels = vec![
        "feature".to_string(),
        "board".to_string(),
        "kanban".to_string(),
    ];
    let out = format_list_long(
        std::slice::from_ref(&it),
        80,
        ListLongOptions::new(true, true),
    );
    let lines: Vec<&str> = out.lines().collect();

    for line in &lines {
        assert!(
            display_width(line) <= 80,
            "line exceeds width 80 ({}): {line:?}",
            display_width(line)
        );
    }
    let header = lines[0];
    assert!(
        header.contains("UPDATED"),
        "header keeps last column intact: {header:?}"
    );
    // Dates (both creation and update) are displayed in full. item() uses epoch.
    let row = lines[1];
    assert_eq!(
        row.matches("1970-01-01").count(),
        2,
        "both dates intact: {row:?}"
    );
    assert!(row.contains("in-progress"), "status intact: {row:?}");
}

#[test]
fn truncate_measures_display_width_for_fullwidth_chars() {
    // Full-width characters are converted to 2 digits. "Aiueo" has a display width of 10. If max=6, 2 characters (width 4) + … = width 5.
    let out = truncate("あいうえお", 6);
    assert_eq!(out, "あい…", "got {out:?}");
    assert_eq!(display_width(&out), 5, "fits within 6 columns");
}

#[test]
fn truncate_leaves_short_strings_untouched() {
    assert_eq!(truncate("abc", 10), "abc");
    assert_eq!(truncate("あいう", 6), "あいう"); // Width 6 = max, not truncated.
}

#[test]
fn display_width_counts_halfwidth_as_one_and_fullwidth_as_two() {
    assert_eq!(display_width("abc"), 3);
    assert_eq!(display_width("あ"), 2);
    assert_eq!(display_width("a１"), 3); // The full-width number '1' is 2 digits.
}

#[test]
fn display_width_treats_combining_marks_as_zero() {
    // U+0301 (Combined Acute Accent) has a display width of 0. 'e' + join = 1 digit.
    assert_eq!(display_width("e\u{0301}"), 1);
}

#[test]
fn display_width_treats_emoji_as_two() {
    // Emoji (U+1F600) is 2 digits equivalent to full-width characters.
    assert_eq!(display_width("😀"), 2);
}

fn detail(item: BacklogItem) -> ItemDetail {
    ItemDetail {
        item,
        rank_ordinal: 1,
        children: vec![],
        dependents: vec![],
    }
}

#[test]
fn format_detail_appends_common_dod_section_after_body() {
    let mut it = item(1, "Task", "todo");
    it.body = "- [ ] item AC".to_string();
    let out = format_detail(
        &detail(it),
        Some("- [ ] common DoD"),
        DetailOptions {
            markdown: false,
            width: 80,
            color: false,
            timezone: DisplayTimezone::Local,
        },
    );
    assert!(out.contains("- [ ] item AC"), "keeps item body: {out}");
    assert!(
        out.contains("## Definition of Done (common)"),
        "adds heading: {out}"
    );
    assert!(out.contains("- [ ] common DoD"), "adds DoD body: {out}");
    // The common DoD follows the item's body.
    assert!(out.find("item AC").unwrap() < out.find("common DoD").unwrap());
}

#[test]
fn format_detail_renders_markdown_body_when_enabled() {
    let mut it = item(1, "Task", "todo");
    it.body = "# Heading\n\n**bold** text".to_string();
    let out = format_detail(
        &detail(it),
        None,
        DetailOptions {
            markdown: true,
            width: 80,
            color: false,
            timezone: DisplayTimezone::Local,
        },
    );
    // Markdown syntax is rendered away; the text content remains.
    assert!(out.contains("Heading"), "keeps heading text: {out}");
    assert!(!out.contains("# Heading"), "strips heading syntax: {out}");
    assert!(!out.contains("**bold**"), "strips emphasis syntax: {out}");
}

#[test]
fn format_detail_keeps_plain_body_when_markdown_disabled() {
    let mut it = item(1, "Task", "todo");
    it.body = "# Heading\n\n**bold** text".to_string();
    let out = format_detail(
        &detail(it),
        None,
        DetailOptions {
            markdown: false,
            width: 80,
            color: false,
            timezone: DisplayTimezone::Local,
        },
    );
    // Opt-out keeps the raw Markdown verbatim.
    assert!(out.contains("# Heading"), "keeps raw heading: {out}");
    assert!(out.contains("**bold**"), "keeps raw emphasis: {out}");
}

#[test]
fn format_detail_omits_dod_section_when_none() {
    let out = format_detail(
        &detail(item(1, "Task", "todo")),
        None,
        DetailOptions {
            markdown: false,
            width: 80,
            color: false,
            timezone: DisplayTimezone::Local,
        },
    );
    assert!(
        !out.contains("Definition of Done"),
        "no DoD section when unset: {out}"
    );
}

#[test]
fn format_detail_uses_configured_timezone_for_all_human_timestamps() {
    let instant = DateTime::<Utc>::from_timestamp(0, 0).expect("timestamp");
    let mut it = item(1, "Task", "todo");
    it.start_at = Some(instant);
    it.done_at = Some(instant);
    it.created = instant;
    it.updated = instant;
    let out = format_detail(
        &detail(it),
        None,
        DetailOptions {
            markdown: false,
            width: 80,
            color: false,
            timezone: "+09:00".parse().expect("offset"),
        },
    );
    assert_eq!(
        out.matches("1970-01-01T09:00:00+09:00").count(),
        4,
        "all four timestamps use the configured offset: {out}"
    );
}

#[test]
pub(super) fn format_board_groups_items_under_columns_in_order() {
    let board = Board {
        columns: vec![
            BoardColumn {
                status: Status::new("todo"),
                items: vec![item(1, "First", "todo"), item(3, "Third", "todo")],
            },
            BoardColumn {
                status: Status::new("in-progress"),
                items: vec![item(2, "Second", "in-progress")],
            },
            BoardColumn {
                status: Status::new("done"),
                items: vec![],
            },
        ],
        orphaned: vec![],
    };

    let expected = "\
todo (2)
  T-1  First
  T-3  Third

in-progress (1)
  T-2  Second

done (0)
  (empty)
";
    assert_eq!(format_board(&board, 80), expected);
}

#[test]
pub(super) fn format_board_follows_terminal_width() {
    let long = "x".repeat(200);
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items: vec![item(1, &long, "todo")],
        }],
        orphaned: vec![],
    };

    // Each line does not exceed the specified width (based on display width) on both narrow and wide terminals.
    let narrow = format_board(&board, 40);
    let wide = format_board(&board, 100);
    let narrow_w = narrow.lines().map(display_width).max().unwrap();
    let wide_w = wide.lines().map(display_width).max().unwrap();
    assert!(narrow.contains('…') && wide.contains('…'), "both truncate");
    assert!(
        narrow_w <= 40,
        "narrow line too wide ({narrow_w}): {narrow}"
    );
    assert!(wide_w <= 100, "wide line too wide ({wide_w}): {wide}");
    // Follows the width of the device, showing more information the wider it is.
    assert!(
        wide_w > narrow_w,
        "wider terminal shows more (fixed width?)"
    );
}

#[test]
fn format_board_with_unbounded_width_never_truncates() {
    // `--no-truncate` displays the entire text with virtually unlimited width (usize::MAX).
    let long = "x".repeat(300);
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items: vec![item(1, &long, "todo")],
        }],
        orphaned: vec![],
    };
    let out = format_board(&board, usize::MAX);
    assert!(out.contains(&long), "shows full title");
    assert!(!out.contains('…'), "no ellipsis: {out}");
}

#[test]
fn format_board_truncates_fullwidth_titles_by_display_width() {
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items: vec![item(1, &"あ".repeat(80), "todo")],
        }],
        orphaned: vec![],
    };
    let out = format_board(&board, 40);
    let widest = out.lines().map(display_width).max().unwrap();
    assert!(out.contains('…'), "fullwidth title truncated: {out}");
    assert!(widest <= 40, "fullwidth line too wide ({widest}): {out}");
}

#[test]
pub(super) fn format_sprints_lists_id_state_and_name() {
    use pinto::sprint::{Sprint, SprintId};
    let now = DateTime::<Utc>::from_timestamp(0, 0).expect("valid epoch");
    let planned = Sprint::new(SprintId::new("S-1").unwrap(), "Planning", now).unwrap();
    let mut active = Sprint::new(SprintId::new("sprint-2").unwrap(), "Build", now).unwrap();
    active.goal = "Ship the sprint".to_string();
    active.start(now).unwrap();

    let out = format_sprints_with_timezone(&[planned, active], DisplayTimezone::Local);

    let expected = "\
S-1       planned  Planning
sprint-2  active   Build  goal: Ship the sprint
";
    assert_eq!(out, expected);
}

#[test]
pub(super) fn format_sprints_shows_period_with_minute_precision() {
    use pinto::sprint::{Sprint, SprintId};
    let now = DateTime::<Utc>::from_timestamp(0, 0).expect("valid epoch");
    let mut s = Sprint::new(SprintId::new("S-1").unwrap(), "Checkout", now).unwrap();
    s.start = Some(
        chrono::NaiveDate::from_ymd_opt(2026, 7, 6)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap()
            .and_utc(),
    );
    s.end = Some(
        chrono::NaiveDate::from_ymd_opt(2026, 7, 17)
            .unwrap()
            .and_hms_opt(18, 15, 0)
            .unwrap()
            .and_utc(),
    );

    let out = format_sprints_with_timezone(&[s], DisplayTimezone::Utc);
    assert!(
        out.contains("(2026-07-06 09:30 → 2026-07-17 18:15)"),
        "period shows minutes: {out}"
    );
}

#[test]
fn format_sprints_uses_configured_timezone_for_periods() {
    use pinto::sprint::{Sprint, SprintId};
    let now = DateTime::<Utc>::from_timestamp(0, 0).expect("valid epoch");
    let mut sprint = Sprint::new(SprintId::new("S-1").unwrap(), "Boundary", now).unwrap();
    sprint.start = Some(now);
    sprint.end = Some(now);

    let out = format_sprints_with_timezone(
        &[sprint],
        "+09:00".parse::<DisplayTimezone>().expect("offset"),
    );

    assert!(
        out.contains("(1970-01-01 09:00 → 1970-01-01 09:00)"),
        "period uses configured timezone: {out}"
    );
}

#[test]
pub(super) fn format_board_lists_orphaned_items_under_undefined_columns_section() {
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items: vec![item(1, "Kept", "todo")],
        }],
        orphaned: vec![item(2, "Stranded", "review")],
    };

    let expected = "\
todo (1)
  T-1  Kept

(!) undefined columns (1)
  T-2  Stranded  [review]
";
    assert_eq!(format_board(&board, 80), expected);
}

// --- Tree display ---

fn with_parent(mut it: BacklogItem, parent: u32) -> BacklogItem {
    it.parent = Some(ItemId::try_new("T", parent).expect("safe test ID"));
    it
}

fn with_deps(mut it: BacklogItem, deps: &[u32]) -> BacklogItem {
    it.depends_on = deps
        .iter()
        .map(|&n| ItemId::try_new("T", n).expect("safe test ID"))
        .collect();
    it
}

#[test]
fn format_board_renders_parent_child_as_tree() {
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items: vec![
                item(1, "Epic", "todo"),
                with_parent(item(2, "Story", "todo"), 1),
                with_parent(item(3, "Task", "todo"), 2),
            ],
        }],
        orphaned: vec![],
    };

    let expected = "\
todo (3)
  T-1  Epic
  └─ T-2  Story
     └─ T-3  Task
";
    assert_eq!(format_board(&board, 80), expected);
}

#[test]
fn format_board_renders_dependencies_as_flat_markers_not_tree() {
    // T-2 and T-3 depend on T-1. Dependencies are not nested in the tree, each item remains flat.
    // Represented by a dependent marker line (⊸/⊷).
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items: vec![
                item(1, "Base", "todo"),
                with_deps(item(2, "Dependent A", "todo"), &[1]),
                with_deps(item(3, "Dependent B", "todo"), &[1]),
            ],
        }],
        orphaned: vec![],
    };

    let out = format_board(&board, 80);
    // Tree borders (├─/└─) are not used, and all three items are lined up flat.
    assert!(
        !out.contains('├') && !out.contains('└'),
        "no tree lines: {out}"
    );
    assert!(out.contains("  T-1  Base\n"), "base flat: {out}");
    assert!(out.contains("  T-2  Dependent A\n"), "dep A flat: {out}");
    assert!(out.contains("  T-3  Dependent B\n"), "dep B flat: {out}");
    // T-2/T-3 appears in the dependents marker of T-1.
    assert!(out.contains("⊷ T-2 T-3"), "T-1 dependents marker: {out}");
    // T-1 appears on the dependencies_on marker of T-2/T-3 (blocking because it is incomplete).
    assert_eq!(
        out.matches("⊸! T-1").count(),
        2,
        "both dependents show blocked depends-on marker: {out}"
    );
}

#[test]
fn format_board_leaves_unrelated_items_flat() {
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items: vec![item(1, "A", "todo"), item(2, "B", "todo")],
        }],
        orphaned: vec![],
    };

    let expected = "\
todo (2)
  T-1  A
  T-2  B
";
    assert_eq!(format_board(&board, 80), expected);
}

#[test]
fn format_board_ignores_cross_column_relationships() {
    // If the parent is in another column, it is not nested and is treated as the root of that column.
    let board = Board {
        columns: vec![
            BoardColumn {
                status: Status::new("todo"),
                items: vec![with_parent(item(2, "Child", "todo"), 1)],
            },
            BoardColumn {
                status: Status::new("done"),
                items: vec![item(1, "Parent", "done")],
            },
        ],
        orphaned: vec![],
    };

    let out = format_board(&board, 80);
    // Child of todo column is flat (parent T-1 is not nested because it is done column).
    assert!(out.contains("todo (1)\n  T-2  Child\n"), "got:\n{out}");
}

#[test]
fn format_board_handles_dependency_cycles_without_looping() {
    // Even when T-1 and T-2 depend on each other, rendering must terminate and show each once.
    // Dependencies are markers, not nested tree edges.
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items: vec![
                with_deps(item(1, "A", "todo"), &[2]),
                with_deps(item(2, "B", "todo"), &[1]),
            ],
        }],
        orphaned: vec![],
    };

    let out = format_board(&board, 80);
    assert_eq!(out.matches("T-1  A").count(), 1, "T-1 shown once: {out}");
    assert_eq!(out.matches("T-2  B").count(), 1, "T-2 shown once: {out}");
    // Mutual dependence: Both parties can both depend on and depend on (blocking).
    assert_eq!(out.matches("⊸! T-2").count(), 1, "A depends on B: {out}");
    assert_eq!(out.matches("⊸! T-1").count(), 1, "B depends on A: {out}");
}

// --- Dependency marker display ---

fn done(mut it: BacklogItem) -> BacklogItem {
    it.done_at = Some(chrono::Utc::now());
    it
}

#[test]
fn format_board_shows_dependency_markers_with_legend() {
    // A dependent item points to an incomplete dependency, so markers and the legend appear.
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items: vec![
                item(1, "Base", "todo"),
                with_deps(item(2, "Dependent", "todo"), &[1]),
            ],
        }],
        orphaned: vec![],
    };

    let out = format_board(&board, 80);
    // The legend uses the same localized wording as kanban.
    let legend = dependency_legend(current());
    assert!(
        out.starts_with(&format!("{legend}\n\n")),
        "shared legend header: {out}"
    );
    assert!(out.contains("⊷ T-2"), "T-1 dependents marker: {out}");
    assert!(
        out.contains("⊸! T-1"),
        "T-2 blocked depends-on marker: {out}"
    );
}

#[test]
fn format_board_omits_legend_when_no_dependencies_exist() {
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items: vec![item(1, "A", "todo"), item(2, "B", "todo")],
        }],
        orphaned: vec![],
    };

    let out = format_board(&board, 80);
    assert!(
        !out.contains('⊸') && !out.contains('⊷'),
        "no markers: {out}"
    );
    assert!(!out.contains("依存先"), "no legend: {out}");
    let expected = "\
todo (2)
  T-1  A
  T-2  B
";
    assert_eq!(out, expected, "unchanged noise-free output: {out}");
}

#[test]
fn format_board_omits_legend_when_marker_chars_appear_only_in_titles() {
    // A legend will not be displayed for boards that only include `⊸` / `⊷` in the title but have no dependencies.
    // (Display of the legend is determined by data, not by string search).
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items: vec![item(1, "記号 ⊸ と ⊷ を含むが依存なし", "todo")],
        }],
        orphaned: vec![],
    };

    let out = format_board(&board, 80);
    let legend = dependency_legend(current());
    assert!(
        !out.contains(&legend),
        "no legend for marker chars in titles: {out}"
    );
}

#[test]
fn format_board_marks_blocked_vs_resolved_dependencies() {
    // T-2 depends on unfinished T-1 (blocked) / T-4 depends on completed T-3 (resolved).
    let board = Board {
        columns: vec![
            BoardColumn {
                status: Status::new("todo"),
                items: vec![
                    item(1, "Base", "todo"),
                    with_deps(item(2, "Blocked", "todo"), &[1]),
                ],
            },
            BoardColumn {
                status: Status::new("done"),
                items: vec![done(item(3, "Finished base", "done"))],
            },
        ],
        orphaned: vec![with_deps(item(4, "Unblocked", "todo"), &[3])],
    };

    let out = format_board(&board, 80);
    assert!(out.contains("⊸! T-1"), "blocked marker uses '!': {out}");
    assert!(
        out.contains("⊸ T-3") && !out.contains("⊸! T-3"),
        "resolved dependency has no '!': {out}"
    );
}

#[test]
fn format_board_truncates_dependency_ids_beyond_limit() {
    // The number of dependent sources is 4 (maximum 3) → the first 3 dependencies + `+1` are abbreviated.
    let mut base = item(1, "Base", "todo");
    base.id = ItemId::try_new("T", 1).expect("safe test ID");
    let dependents: Vec<BacklogItem> = (2..=5)
        .map(|n| with_deps(item(n, &format!("Dep {n}"), "todo"), &[1]))
        .collect();
    let mut items = vec![base];
    items.extend(dependents);
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items,
        }],
        orphaned: vec![],
    };

    let out = format_board(&board, 80);
    assert!(
        out.contains("⊷ T-2 T-3 T-4 +1"),
        "dependents truncated with +N: {out}"
    );
    // Omitting `+N` for the number of IDs is independent of the display width, so even `--no-truncate` (unlimited width)
    // continues to be applied (as opposed to suppressing the ellipsis `…` by width).
    let unbounded = format_board(&board, usize::MAX);
    assert!(
        unbounded.contains("⊷ T-2 T-3 T-4 +1"),
        "id count truncation still applies under --no-truncate: {unbounded}"
    );
}

#[test]
fn format_board_keeps_parent_child_tree_with_dependency_markers() {
    // The parent-child tree (├─/└─) is maintained, and if there is a dependency, a marker line is added below it.
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items: vec![
                item(1, "Base", "todo"),
                with_deps(with_parent(item(2, "Story", "todo"), 1), &[1]),
            ],
        }],
        orphaned: vec![],
    };

    let out = format_board(&board, 80);
    assert!(out.contains("  T-1  Base\n"), "root flat: {out}");
    assert!(
        out.contains("  └─ T-2  Story\n"),
        "parent-child tree kept: {out}"
    );
    // The marker line appears at a position that inherits the tree's indentation (rule width).
    assert!(
        out.contains("     ⊸! T-1\n") || out.contains("     ⊸! T-1  ⊷"),
        "marker aligned under child title: {out:?}"
    );
}

#[test]
fn format_board_marks_orphaned_items_too() {
    let board = Board {
        columns: vec![],
        orphaned: vec![
            item(1, "Base", "review"),
            with_deps(item(2, "Dependent", "review"), &[1]),
        ],
    };

    let out = format_board(&board, 80);
    assert!(out.contains("⊷ T-2"), "orphaned dependents marker: {out}");
    assert!(out.contains("⊸! T-1"), "orphaned depends-on marker: {out}");
}

#[test]
fn format_board_truncates_dependency_marker_line_by_width() {
    let mut base = item(1, &"x".repeat(200), "todo");
    base.id = ItemId::try_new("T", 1).expect("safe test ID");
    let dependents: Vec<BacklogItem> = (2..=9)
        .map(|n| with_deps(item(n, "d", "todo"), &[1]))
        .collect();
    let mut items = vec![base];
    items.extend(dependents);
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items,
        }],
        orphaned: vec![],
    };

    let out = format_board(&board, 40);
    let legend = dependency_legend(current());
    // Legends are not subject to width restrictions. Validate only cards and dependent marker lines.
    for line in out.lines().filter(|line| *line != legend) {
        assert!(display_width(line) <= 40, "line exceeds width 40: {line:?}");
    }
}

#[test]
fn format_board_with_unbounded_width_does_not_truncate_dependency_marker_line() {
    // With `--no-truncate` (usize::MAX), dependent marker lines are also not marked with an ellipsis (...) depending on their width.
    // However, the `+N` omission of the ID count (DEP_ID_LIMIT) is a mechanism independent of the display width, so
    // If the number of dependent sources exceeds the upper limit, `+N` is added as usual (here, only one item within the upper limit).
    let mut base = item(1, "Base", "todo");
    base.id = ItemId::try_new("T", 1).expect("safe test ID");
    let dependent = with_deps(item(2, &"y".repeat(200), "todo"), &[1]);
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items: vec![base, dependent],
        }],
        orphaned: vec![],
    };

    let out = format_board(&board, usize::MAX);
    assert!(!out.contains('…'), "no ellipsis in unbounded width: {out}");
    assert!(out.contains("⊸! T-1"), "marker not truncated away: {out}");
}

#[test]
fn format_board_with_zero_width_falls_back_to_min_title_width() {
    // Even if the width is extremely small (0), there will be no panic or infinite loop, and it will also go to the marker line as well as the title.
    // The lower limit of MIN_TITLE_WIDTH is effective (variable parts excluding fixed parts are drawn with the lower limit width).
    let mut base = item(1, "Base", "todo");
    base.id = ItemId::try_new("T", 1).expect("safe test ID");
    let dependents: Vec<BacklogItem> = (2..=9)
        .map(|n| with_deps(item(n, "d", "todo"), &[1]))
        .collect();
    let mut items = vec![base];
    items.extend(dependents);
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items,
        }],
        orphaned: vec![],
    };

    let out = format_board(&board, 0);
    let legend = dependency_legend(current());
    // Short markers (width ≤ MIN_TITLE_WIDTH) remain.
    assert!(out.contains("⊸! T-1"), "short marker survives: {out}");
    // Long markers (8 dependent sources) are omitted at the lower limit width. The fixed part is ` {id:3} ` (width 7).
    // The legend line at the beginning (fixed header, not subject to width restrictions) is excluded from inspection.
    let dependents_line = out
        .lines()
        .find(|l| l.contains('⊷') && *l != legend)
        .unwrap_or_else(|| panic!("dependents marker line exists: {out}"));
    assert!(
        dependents_line.contains('…'),
        "truncated: {dependents_line:?}"
    );
    assert!(
        display_width(dependents_line) <= 7 + MIN_TITLE_WIDTH,
        "marker line bounded by MIN_TITLE_WIDTH floor: {dependents_line:?}"
    );
}

#[test]
fn format_board_narrower_than_fixed_prefix_keeps_min_title_width() {
    // Even if the width is narrower than the fixed part (` {id} ` = width 7), it will be drawn with the lower limit MIN_TITLE_WIDTH and will not collapse.
    let mut base = item(1, "Base", "todo");
    base.id = ItemId::try_new("T", 1).expect("safe test ID");
    let dependents: Vec<BacklogItem> = (2..=9)
        .map(|n| with_deps(item(n, "d", "todo"), &[1]))
        .collect();
    let mut items = vec![base];
    items.extend(dependents);
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items,
        }],
        orphaned: vec![],
    };

    let out = format_board(&board, 5);
    let legend = dependency_legend(current());
    assert!(out.contains("⊸! T-1"), "short marker survives: {out}");
    // The legend line at the beginning (fixed header, not subject to width restrictions) is excluded from inspection.
    for line in out
        .lines()
        .filter(|l| (l.contains('⊷') || l.contains('⊸')) && *l != legend)
    {
        assert!(
            display_width(line) <= 7 + MIN_TITLE_WIDTH,
            "marker line bounded by MIN_TITLE_WIDTH floor: {line:?}"
        );
    }
}

#[test]
fn format_board_handles_fullwidth_titles_with_markers_at_small_width() {
    // Even if the title + dependent marker contains full-width characters, the width calculation will not fail and each line will fit within the width.
    let mut base = item(1, "全角の長いタイトルを持つ基盤タスク", "todo");
    base.id = ItemId::try_new("T", 1).expect("safe test ID");
    let dependent = with_deps(item(2, "依存する全角タイトルのタスク", "todo"), &[1]);
    let board = Board {
        columns: vec![BoardColumn {
            status: Status::new("todo"),
            items: vec![base, dependent],
        }],
        orphaned: vec![],
    };

    let out = format_board(&board, 20);
    let legend = dependency_legend(current());
    assert!(out.contains('…'), "fullwidth titles truncated: {out}");
    assert!(out.contains("⊸! T-1"), "marker rendered: {out}");
    assert!(out.contains("⊷ T-2"), "dependents marker rendered: {out}");
    // The legend line at the beginning (fixed header, not subject to width restrictions) is excluded from inspection.
    for line in out.lines().filter(|l| *l != legend) {
        assert!(
            display_width(line) <= 20,
            "line within width 20 despite fullwidth chars: {line:?}"
        );
    }
}

// --- Burndown drawing ---

fn sample_burndown() -> Burndown {
    use pinto::service::{BurndownDay, BurndownMetric};
    use pinto::sprint::SprintId;
    let d = |m, day| chrono::NaiveDate::from_ymd_opt(2026, m, day).unwrap();
    Burndown {
        sprint_id: SprintId::new("S-1").unwrap(),
        sprint_title: "Sprint One".to_string(),
        metric: BurndownMetric::Points,
        total: 8,
        days: vec![
            BurndownDay {
                date: d(7, 6),
                remaining: 8,
                ideal: 8.0,
            },
            BurndownDay {
                date: d(7, 7),
                remaining: 5,
                ideal: 4.0,
            },
            BurndownDay {
                date: d(7, 8),
                remaining: 0,
                ideal: 0.0,
            },
        ],
    }
}

#[test]
fn format_burndown_wide_draws_bars_within_width() {
    let out = format_burndown(&sample_burndown(), 80);
    assert!(
        out.contains("S-1 Sprint One — burndown (points)"),
        "header: {out}"
    );
    assert!(
        out.contains("Period 2026-07-06 → 2026-07-08"),
        "period: {out}"
    );
    assert!(out.contains('█'), "draws bars: {out}");
    assert!(out.contains('┆'), "draws ideal marker: {out}");
    // The daily bar line fits within the width of 80 (headings such as titles are not included).
    for line in out.lines().filter(|l| l.starts_with("2026-")) {
        assert!(line.width() <= 80, "line exceeds width: {line:?}");
    }
}

#[test]
fn format_burndown_narrow_falls_back_to_numeric_table() {
    let out = format_burndown(&sample_burndown(), 22);
    assert!(!out.contains('█'), "no bars in narrow mode: {out}");
    assert!(out.contains("rem"), "table header has rem: {out}");
    assert!(out.contains("ideal"), "table header has ideal: {out}");
    // The remaining amount for each date is displayed as a numerical value.
    assert!(out.contains("2026-07-06"), "lists dates: {out}");
    // Daily lines fit within width 22 (headings such as titles are not included).
    for line in out.lines().filter(|l| l.starts_with("2026-")) {
        assert!(line.width() <= 22, "line exceeds width: {line:?}");
    }
}

#[test]
fn format_duration_shows_up_to_two_units() {
    assert_eq!(format_duration(Duration::zero()), "0s");
    assert_eq!(format_duration(Duration::seconds(30)), "30s");
    assert_eq!(format_duration(Duration::minutes(45)), "45m");
    assert_eq!(format_duration(Duration::seconds(90)), "1m 30s");
    assert_eq!(format_duration(Duration::hours(12)), "12h");
    assert_eq!(format_duration(Duration::minutes(150)), "2h 30m");
    assert_eq!(format_duration(Duration::days(2)), "2d");
    assert_eq!(format_duration(Duration::hours(52)), "2d 4h");
    assert_eq!(format_duration(Duration::hours(-5)), "-5h");
}

fn sample_summary() -> DurationSummary {
    DurationSummary {
        count: 2,
        mean: Duration::hours(36),
        median: Duration::hours(30),
        min: Duration::hours(24),
        max: Duration::hours(48),
    }
}

#[test]
fn format_cycletime_lists_both_metrics_and_missing_warning() {
    let report = CycleTimeReport {
        cycle: Some(sample_summary()),
        lead: Some(sample_summary()),
        missing_start: vec![ItemId::try_new("T", 7).expect("safe test ID")],
        completed: 3,
    };
    let out = format_cycletime(&report);
    assert!(out.contains("3 completed"), "shows completed count: {out}");
    assert!(out.contains("cycle (start → done)"), "cycle line: {out}");
    assert!(out.contains("lead  (created → done)"), "lead line: {out}");
    assert!(out.contains("mean 1d 12h"), "humanizes durations: {out}");
    assert!(
        out.contains("T-7") && out.contains("without start time"),
        "warns about missing start: {out}"
    );
}

#[test]
fn format_cycletime_reports_no_completed_items() {
    let report = CycleTimeReport {
        cycle: None,
        lead: None,
        missing_start: Vec::new(),
        completed: 0,
    };
    let out = format_cycletime(&report);
    assert!(out.contains("0 completed"), "states none completed: {out}");
    assert!(!out.contains("cycle ("), "no metric lines: {out}");
}
