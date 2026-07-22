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

The script builds and runs the `large_board_bench` example, which reports the median of three samples for `list`, `show`, `add`, and
`move` at both required board sizes. See [`docs/benchmarks.md`](../../../docs/benchmarks.md)
and [`docs/stability.md`](../../../docs/stability.md) for the method and the
decision to retain complete-board fail-fast validation for single-item reads.
