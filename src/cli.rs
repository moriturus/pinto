//! CLI layer.
//!
//! Keep argument definitions, command execution, and output formatting separate; `main.rs` only
//! starts the application.

mod args;
mod commands;
mod dependency_display;
mod editor;
mod format;
mod json;
mod kanban;
mod markdown;
mod shell;

pub(crate) use commands::entrypoint;
