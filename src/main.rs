//! pinto CLI entry point.
//!
//! Argument parsing and command execution are delegated to the `cli` module; `main` only starts
//! the Tokio runtime.

mod cli;

use std::process::ExitCode;

/// The derived clap command tree can need more stack than the Windows process default when it is
/// used for completion generation or an interactive shell.
const CLI_THREAD_STACK_SIZE: usize = 8 * 1024 * 1024;

fn main() -> ExitCode {
    let thread = std::thread::Builder::new()
        .name("pinto-main".to_string())
        .stack_size(CLI_THREAD_STACK_SIZE)
        .spawn(|| {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("pinto: failed to start the async runtime: {error}");
                    return ExitCode::from(2);
                }
            };
            runtime.block_on(cli::entrypoint())
        });

    match thread {
        Ok(thread) => match thread.join() {
            Ok(code) => code,
            Err(_) => {
                eprintln!("pinto: the CLI thread panicked");
                ExitCode::from(2)
            }
        },
        Err(error) => {
            eprintln!("pinto: failed to start the CLI thread: {error}");
            ExitCode::from(2)
        }
    }
}
