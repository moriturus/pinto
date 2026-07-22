# Large-board file-backend benchmarks

`large_board_bench` measures the file backend's command latency on generated
boards containing 1,000 and 10,000 PBIs. The example writes an export-compatible
snapshot in a temporary directory, then uses `cargo run` for initialization,
import, warm-up, and every measured command. It never edits the repository's
`.pinto` board. `list` and `show` use `--json`; `add` and `move` capture their
output so terminal rendering does not affect the result.

Run the benchmark with the pinned lockfile and release profile:

```bash
./scripts/large-board-benchmark.sh
```

The script builds the runner first, then launches it directly so its child
`cargo run` commands do not contend with the outer Cargo build lock.

The default run takes three samples per board size and reports the median. The
first `list` invocation warms the cargo-run build path; the reported timings
still include process startup and cargo-run dispatch because those are part of
the user-facing command workflow. For a quick smoke run, select smaller boards:

```bash
./scripts/large-board-benchmark.sh \
  --sizes 100,200 --samples 1
```

The result below was collected on 2026-07-22 in the repository development
environment with the pinned Rust toolchain (`mise.toml`). Values are medians;
they are comparative measurements rather than performance guarantees.

| Items | `list` | `show` | `add` | `move` |
| ---: | ---: | ---: | ---: | ---: |
| 1,000 | 272.0 ms | 264.3 ms | 307.5 ms | 341.1 ms |
| 10,000 | 486.1 ms | 501.3 ms | 1,013.1 ms | 1,138.5 ms |

The complete-board validation contract makes the single-item `show` path scale
with the task and archive set, just like `list`. See the scaling decision in
[`docs/stability.md`](stability.md).

## Rank benchmarks

The `rank_bench` example measures the fractional-indexing operations used to
place PBIs between neighboring items. Run it with:

```bash
cargo run --example rank_bench --release
```

Benchmark results are environment-dependent. Compare runs on the same machine
and toolchain, and focus on regressions rather than absolute numbers. The rank
implementation favors predictable ordering and bounded maintenance work; a
rebalance operation shortens only oversized sibling scopes when ranks become
unnecessarily long.
