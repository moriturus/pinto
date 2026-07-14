//! pinto CLI entry point.
//!
//! Argument parsing and command execution are delegated to the `cli` module; `main` only starts
//! the Tokio runtime.

mod cli;

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    cli::entrypoint().await
}
