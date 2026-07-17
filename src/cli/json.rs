//! Machine-readable `--json` output.
//!
//! Convert domain types such as [`BacklogItem`] and [`Sprint`] to the stable JSON schema promised
//! by the CLI. Dedicated DTOs keep internal domain refactors from changing script-facing output.
//!
//! For backward compatibility policy, see [`docs/json-schema.md`](../../docs/json-schema.md). In summary:
//! Deleting, renaming, or changing the type of an existing key is a breaking change; adding keys
//! is non-destructive. Optional fields always appear and use `null` when unset.

use pinto::backlog::{BacklogItem, ItemId};
use pinto::service::{Board, Burndown, CycleTimeReport, DurationSummary, ItemDetail};
use pinto::sprint::{Sprint, SprintCapacity};
use serde::Serialize;

/// JSON representation of a backlog item.
///
/// Dates and times use RFC3339 strings. Optional fields always appear as keys and use `null` when
/// unset; collection fields use an empty array.
#[derive(Debug, Serialize)]
struct ItemJson {
    id: String,
    title: String,
    status: String,
    rank: String,
    points: Option<u32>,
    labels: Vec<String>,
    assignee: Option<String>,
    sprint: Option<String>,
    parent: Option<String>,
    depends_on: Vec<String>,
    start_at: Option<String>,
    done_at: Option<String>,
    commits: Vec<String>,
    created: String,
    updated: String,
    body: String,
}

impl ItemJson {
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
            start_at: item.start_at.map(|d| d.to_rfc3339()),
            done_at: item.done_at.map(|d| d.to_rfc3339()),
            commits: item.commits.clone(),
            created: item.created.to_rfc3339(),
            updated: item.updated.to_rfc3339(),
            body: item.body.clone(),
        }
    }
}

/// JSON representation of PBI details (`show`), with bidirectional links as flat fields.
#[derive(Debug, Serialize)]
struct DetailJson {
    #[serde(flatten)]
    item: ItemJson,
    /// 1-based sibling ordinal for display, in ascending rank order within the same column.
    rank_ordinal: usize,
    /// IDs of child items parented by this PBI (in ascending rank order).
    children: Vec<String>,
    /// IDs of items that depend on this PBI (in ascending rank order).
    dependents: Vec<String>,
}

/// A single column JSON representation of the board.
#[derive(Debug, Serialize)]
struct ColumnJson {
    status: String,
    items: Vec<ItemJson>,
}

/// A JSON representation of the board (`board`).
#[derive(Debug, Serialize)]
struct BoardJson {
    columns: Vec<ColumnJson>,
    /// PBIs whose status is absent from `config.toml`.
    orphaned: Vec<ItemJson>,
}

/// JSON representation of a sprint (`sprint list`).
#[derive(Debug, Serialize)]
struct SprintJson {
    id: String,
    title: String,
    state: String,
    goal: String,
    start: Option<String>,
    end: Option<String>,
    closed_at: Option<String>,
    spillover_points: u32,
    spillover_items: u32,
    unestimated_spillover_items: u32,
    created: String,
    updated: String,
}

impl SprintJson {
    fn from_sprint(sprint: &Sprint) -> Self {
        Self {
            id: sprint.id.to_string(),
            title: sprint.title.clone(),
            state: sprint.state.to_string(),
            goal: sprint.goal.clone(),
            start: sprint.start.map(|d| d.to_rfc3339()),
            end: sprint.end.map(|d| d.to_rfc3339()),
            closed_at: sprint.closed_at.map(|d| d.to_rfc3339()),
            spillover_points: sprint.spillover.points,
            spillover_items: sprint.spillover.items,
            unestimated_spillover_items: sprint.spillover.unestimated_items,
            created: sprint.created.to_rfc3339(),
            updated: sprint.updated.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize)]
struct SprintCapacityJson {
    working_days: u32,
    hours: f64,
}

pub(super) fn sprint_capacity_json(capacity: &SprintCapacity) -> serde_json::Result<String> {
    serde_json::to_string_pretty(&SprintCapacityJson {
        working_days: capacity.working_days,
        hours: capacity.hours,
    })
}

/// Format the PBI list into a formatted JSON array. If empty, `[]` (the caller adds a trailing newline).
pub(super) fn list_json(items: &[BacklogItem]) -> serde_json::Result<String> {
    let dto: Vec<ItemJson> = items.iter().map(ItemJson::from_item).collect();
    serde_json::to_string_pretty(&dto)
}

/// Format PBI details into a JSON array (PBI fields + `children` / `dependents`).
pub(super) fn detail_json(details: &[ItemDetail]) -> serde_json::Result<String> {
    let dto: Vec<DetailJson> = details
        .iter()
        .map(|detail| DetailJson {
            item: ItemJson::from_item(&detail.item),
            rank_ordinal: detail.rank_ordinal,
            children: detail.children.iter().map(ItemId::to_string).collect(),
            dependents: detail.dependents.iter().map(ItemId::to_string).collect(),
        })
        .collect();
    serde_json::to_string_pretty(&dto)
}

/// Format the board into a JSON object (`columns` / `orphaned`).
pub(super) fn board_json(board: &Board) -> serde_json::Result<String> {
    let dto = BoardJson {
        columns: board
            .columns
            .iter()
            .map(|col| ColumnJson {
                status: col.status.to_string(),
                items: col.items.iter().map(ItemJson::from_item).collect(),
            })
            .collect(),
        orphaned: board.orphaned.iter().map(ItemJson::from_item).collect(),
    };
    serde_json::to_string_pretty(&dto)
}

/// Format the Sprint list into a JSON array. If empty, `[]`.
pub(super) fn sprints_json(sprints: &[Sprint]) -> serde_json::Result<String> {
    let dto: Vec<SprintJson> = sprints.iter().map(SprintJson::from_sprint).collect();
    serde_json::to_string_pretty(&dto)
}

/// A JSON representation of one day of burndown (`sprint burndown`).
#[derive(Debug, Serialize)]
struct BurndownDayJson {
    date: String,
    remaining: u32,
    ideal: f64,
}

/// JSON representation of burndown aggregation results (Sprint ID/title/metric/total amount/daily series).
#[derive(Debug, Serialize)]
struct BurndownJson {
    sprint_id: String,
    sprint_title: String,
    metric: String,
    total: u32,
    days: Vec<BurndownDayJson>,
}

/// JSON representation of period summary statistics (Cycle/Lead Time).
///
/// Time is expressed in seconds (integer). Human-readable output is formatted as date/time, but machine-readable output is formatted as raw seconds, which is easier to process.
#[derive(Debug, Serialize)]
struct DurationSummaryJson {
    count: usize,
    mean_seconds: i64,
    median_seconds: i64,
    min_seconds: i64,
    max_seconds: i64,
}

impl DurationSummaryJson {
    fn from_summary(s: &DurationSummary) -> Self {
        Self {
            count: s.count,
            mean_seconds: s.mean.num_seconds(),
            median_seconds: s.median.num_seconds(),
            min_seconds: s.min.num_seconds(),
            max_seconds: s.max.num_seconds(),
        }
    }
}

/// JSON representation of Cycle/Lead Time analysis (`cycletime`).
///
/// A summary with no target is `null`. Optional fields always have a key (consumer does not need to branch).
#[derive(Debug, Serialize)]
struct CycleTimeJson {
    completed: usize,
    cycle: Option<DurationSummaryJson>,
    lead: Option<DurationSummaryJson>,
    missing_start: Vec<String>,
}

/// Format the Cycle/Lead Time analysis results to JSON.
pub(super) fn cycletime_json(report: &CycleTimeReport) -> serde_json::Result<String> {
    let dto = CycleTimeJson {
        completed: report.completed,
        cycle: report.cycle.as_ref().map(DurationSummaryJson::from_summary),
        lead: report.lead.as_ref().map(DurationSummaryJson::from_summary),
        missing_start: report.missing_start.iter().map(ItemId::to_string).collect(),
    };
    serde_json::to_string_pretty(&dto)
}

/// Format the burndown aggregation results into JSON.
pub(super) fn burndown_json(chart: &Burndown) -> serde_json::Result<String> {
    let dto = BurndownJson {
        sprint_id: chart.sprint_id.to_string(),
        sprint_title: chart.sprint_title.clone(),
        metric: chart.metric.as_str().to_string(),
        total: chart.total,
        days: chart
            .days
            .iter()
            .map(|d| BurndownDayJson {
                date: d.date.to_string(),
                remaining: d.remaining,
                ideal: d.ideal,
            })
            .collect(),
    };
    serde_json::to_string_pretty(&dto)
}
