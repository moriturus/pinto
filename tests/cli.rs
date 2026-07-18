//! CLI integration tests for the pinto binary.
//!
//! Feature-specific test modules keep failures close to the command under test;
//! shared process and fixture helpers live in `common`.

#[path = "cli/add.rs"]
mod add;
#[path = "cli/automation.rs"]
mod automation;
#[path = "cli/board.rs"]
mod board;
#[path = "cli/common.rs"]
mod common;
#[path = "cli/dependencies.rs"]
mod dependencies;
#[path = "cli/doctor.rs"]
mod doctor;
#[path = "cli/dod.rs"]
mod dod;
#[path = "cli/export.rs"]
mod export;
#[path = "cli/item.rs"]
mod item;
#[path = "cli/kanban.rs"]
mod kanban;
#[path = "cli/list_show.rs"]
mod list_show;
#[path = "cli/misc.rs"]
mod misc;
#[path = "cli/move_reorder.rs"]
mod move_reorder;
#[path = "cli/next.rs"]
mod next;
#[path = "cli/sprint.rs"]
mod sprint;
