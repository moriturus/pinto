# large-board-benchmark (single feature: file-backend scaling)

This small board demonstrates the CLI workflow used by the large-board
benchmark. The benchmark itself generates temporary 1,000-item and 10,000-item
boards through an export/import snapshot, so the repository does not carry a
large fixture.

From this demo directory, inspect and mutate the checked-in board with:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --plain
cargo run --manifest-path ../../../Cargo.toml -- add "Measure another board"
cargo run --manifest-path ../../../Cargo.toml -- move T-1 in-progress
```

From the repository root, run the reproducible benchmark:

```bash
./scripts/large-board-benchmark.sh
```

The script builds and runs the `large_board_bench` example, which reports the
median of three fresh-board samples for `list`, `show`, `add`, `move`, `doctor`,
and `import` at both required board sizes. To run the regression gate and save
the same JSON evidence used in CI:

```bash
./scripts/large-board-benchmark.sh \
  --baseline benchmarks/large-board-baseline.json \
  --tolerance 20 \
  --output target/large-board-benchmark.json
```

The report includes every sample, each median, and its execution environment.
Use the baseline matching the host platform; the CI workflow uses
`benchmarks/large-board-baseline-linux-x86_64.json`, while the command above
uses the macOS/aarch64 local baseline.
See [`docs/benchmarks.md`](../../../docs/benchmarks.md) for the measurement
method, baseline refresh procedure, 14-day CI artifact retention, and
interpretation of environment-dependent results. See
[`docs/stability.md`](../../../docs/stability.md) for the decision to retain
complete-board fail-fast validation for single-item reads.
