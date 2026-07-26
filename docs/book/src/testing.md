# Testing and fuzzing

Run the normal test layers from a checkout with the Rust toolchain selected by
mise:

```bash
mise run test                 # unit, CLI, docs, i18n, and skill integration tests
cargo test --doc --locked     # public API examples
cargo test --test cli --locked # CLI and pseudo-terminal smoke tests
mise run check                # tests, Clippy, Rust docs, mdBook, and fmt
```

`mise run coverage` writes `coverage.xml` in Cobertura format and then checks
the artifact's root Cobertura line-rate with `scripts/check-coverage.sh`. The 0.95
threshold is therefore applied to the same metric that CI uploads, rather than
to the different denominator used by the LLVM text summary.

## Kanban runtime failure-path matrix

The Kanban runtime has a separate Cobertura guard for
`src.cli.kanban.runtime`, because the aggregate line-rate can hide an
under-tested terminal boundary. The focused suite exercises these paths without
requiring a real TTY:

| Failure or boundary | Focused verification |
| --- | --- |
| terminal sizing failure | A fake frame driver returns a size error before drawing or event polling. |
| drawing failure | A fake frame driver returns the drawing error before event polling. |
| event polling failure | A successful frame is followed by a polling error before event reading. |
| event-reading failure | A successful frame is followed by an event-reading error. |
| panic unwinding | The terminal guard restores its lifecycle state while a panic unwinds. |
| narrow terminals | A zero-width terminal still keeps the selected column visible. |
| repeated resize events | Consecutive resize events recompute the horizontal viewport. |

Run the matrix with:

```bash
cargo test --bin pinto --locked cli::kanban::runtime
```

The same runtime package must meet the 0.90 line-rate threshold in
`scripts/check-kanban-coverage.sh`; the repository-wide 0.95 gate remains
unchanged.

The macOS PTY lifecycle regression can be reproduced with:

```bash
cargo test --test cli kanban::pty_tests::shell_can_reenter_kanban_without_leaking_lifecycle_state -- --exact --nocapture
```

The CI failure observed on the macOS 26 arm64 runner occurred after the test
had returned to the third `pinto>` prompt following two Kanban entries. The
child process did not satisfy the test's three-second exit deadline after
Ctrl-D, while the same lifecycle passed on local macOS and Linux; the Windows
check suite also passed.
The test keeps the Ctrl-D and terminal-flag assertions, but uses the
platform-specific `SHELL_EXIT_WAIT` deadline only for this final process-exit
wait so PTY teardown latency is not mistaken for a lifecycle leak.

The Markdown frontmatter parser and automation-plan parser have libFuzzer
targets under `fuzz/`. Install the fuzz runner once with nightly-compatible
tooling:

```bash
rustup toolchain install nightly
cargo install cargo-fuzz --locked
cargo fuzz list
cargo fuzz run automation_plan_parse -- -max_total_time=300
cargo fuzz run markdown_frontmatter_parse -- -max_total_time=300
```

The weekly scheduled CI workflow runs both targets for five minutes and uploads
failures from `fuzz/artifacts`. To reproduce a reported input locally, pass the
uploaded crash file or corpus directory to the same target:

```bash
cargo fuzz run markdown_frontmatter_parse fuzz/artifacts/markdown_frontmatter_parse/crash-...
```

Keep the failing input when fixing a parser bug, then rerun the target with a
short time limit and finish with `mise run check`. The fuzz targets treat parser
errors as expected input outcomes; a panic or sanitizer failure is the failure
signal.
