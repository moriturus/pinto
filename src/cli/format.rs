//! CLI output formatting.

pub(super) mod board;
pub(super) mod item;
pub(super) mod report;
pub(super) mod sprint;
#[cfg(test)]
mod tests;
pub(super) mod text;
pub(super) mod tree;

pub(super) use text::DEFAULT_TERM_WIDTH;
