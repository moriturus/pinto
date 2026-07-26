#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

# Build the CLI and runner before launching the benchmark. The runner invokes the release CLI
# directly for each measured command, so no nested Cargo process affects the timings.
cargo build --release --bin pinto --example large_board_bench --locked

runner="$root/target/release/examples/large_board_bench"
if [ -x "$runner" ]; then
    exec "$runner" "$@"
fi

if [ -x "$runner.exe" ]; then
    exec "$runner.exe" "$@"
fi

echo "large_board_bench executable was not produced at $runner" >&2
exit 1
