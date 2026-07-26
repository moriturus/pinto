# Large-board file-backend benchmarks

`large_board_bench` measures the file backend's command latency on generated
boards containing 1,000 and 10,000 PBIs. For every sample and command it creates
a fresh temporary board, initializes it, and imports the generated
export-compatible snapshot before timing exactly one command. Setup is excluded
from the command timing. The benchmark never edits the repository's `.pinto`
board. `list` and `show` use `--json`; all command output is captured so terminal
rendering does not affect the result.

The measured command set is `list`, `show`, `add`, `move`, `doctor`, and
`import`. The release `pinto` binary is launched directly after the wrapper has
built it, which keeps Cargo build-lock and dispatch overhead out of the
measurement while retaining process-startup and file-backend costs.

Run the benchmark with the pinned lockfile and release profile:

```bash
./scripts/large-board-benchmark.sh
```

The script builds both the release CLI and runner first, then launches the
runner directly. For a run that also checks the committed recent baseline and
writes a machine-readable report, use:

```bash
./scripts/large-board-benchmark.sh \
  --baseline benchmarks/large-board-baseline.json \
  --tolerance 20 \
  --output target/large-board-benchmark.json
```

The default run takes three samples per board size and reports the median. The
JSON report stores all sample values, each command median, the selected sizes,
and the execution environment (OS, architecture, Rust toolchain, CI runner,
and commit). For a quick smoke run, select smaller boards; omit `--baseline`
when the selected size is not represented in the committed baseline:

```bash
./scripts/large-board-benchmark.sh --sizes 100,200 --samples 1
```

Regression detection uses a relative comparison rather than a fixed time
limit. For each selected size and command, a run fails only when
`measured_median > baseline_median * (1 + tolerance / 100)`. The default
tolerance is 20%; CI passes it explicitly. A failure names the board size,
command, baseline median, measured median, delta, and measured environment.

Baselines are platform-specific. The committed
[`large-board-baseline.json`](../benchmarks/large-board-baseline.json) is the
macOS/aarch64 reference for local development, while
[`large-board-baseline-linux-x86_64.json`](../benchmarks/large-board-baseline-linux-x86_64.json)
is the Linux/x86_64 reference used by CI. The runner rejects a baseline whose
OS, architecture, or profile differs from the measured environment; this
prevents Docker or cross-platform startup costs from being reported as code
regressions. Refresh the matching baseline only after an intentional
performance change: run the benchmark with the same toolchain and platform,
inspect the stored environment and medians, then replace that baseline in a
focused commit. Do not use a one-off absolute limit to hide a regression.

The baselines were collected with the repository's pinned development
environment; values are medians and are comparative measurements rather than
performance guarantees.

| Items | `list` | `show` | `add` | `move` | `doctor` | `import` |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 35.0 ms | 37.7 ms | 66.2 ms | 77.8 ms | 41.4 ms | 174.8 ms |
| 10,000 | 306.3 ms | 297.5 ms | 615.4 ms | 779.0 ms | 330.6 ms | 1,685.0 ms |

## CI retention

The regular CI path runs the 1,000-item smoke benchmark on Ubuntu for pushes
and pull requests. The scheduled weekly path runs the 10,000-item benchmark;
both use the Linux/x86_64 baseline. Each GitHub Actions job writes its JSON
report to an artifact with a 14-day retention period. When running the jobs
with `act`, the benchmark still runs and writes its report, but the artifact
upload step is skipped because act does not provide GitHub's artifact runtime
token; act also uses a 50% tolerance to account for its emulated local
container startup cost, while GitHub Actions keeps the 20% gate. Inspect the
report's `environment`, `samples_ms`, and `median_ms` fields before comparing
runs; compare measurements from compatible toolchains and runner classes
rather than treating the medians as universal service-level objectives.

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
