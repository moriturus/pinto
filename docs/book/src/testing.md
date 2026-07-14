# Testing and fuzzing

Run the normal test layers from a checkout with the Rust toolchain selected by
mise:

```bash
mise run test                 # unit, CLI, docs, i18n, and skill integration tests
cargo test --doc              # public API examples
cargo test --test cli         # CLI and pseudo-terminal smoke tests
mise run check                # tests, Clippy, Rust docs, mdBook, and fmt
```

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
