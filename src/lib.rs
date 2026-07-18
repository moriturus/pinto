//! pinto — A lightweight CLI/TUI backlog/kanban board for Scrum.
//!
//! This crate provides the domain and service layers used by the CLI (`main.rs`) and TUI.
//! See `docs/DESIGN.md` for detailed design.

// Consumers refer to module paths such as `pinto::backlog::…`; keep the module boundaries explicit
// rather than flattening the crate root. `config` remains private to the crate, while the storage
// facade re-exports the `StorageBackend` type needed by selection and migration APIs.
pub mod automation;
pub mod backlog;
pub mod error;
pub mod i18n;
pub mod kanban_keys;
pub mod rank;
pub mod service;
pub mod sprint;
pub mod storage;
pub mod template;
pub mod timezone;

mod config;
mod user_config;

/// Crate version (synced with `Cargo.toml`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_not_empty() {
        assert!(!VERSION.is_empty());
    }
}
