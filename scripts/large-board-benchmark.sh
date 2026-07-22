#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

# Build before launching the runner. The runner invokes cargo run for each measured command, so
# it must not itself be running under cargo while those child processes need the build lock.
cargo build --release --example large_board_bench --locked

runner="$root/target/release/examples/large_board_bench"
if [ -x "$runner" ]; then
    exec "$runner" "$@"
fi

if [ -x "$runner.exe" ]; then
    exec "$runner.exe" "$@"
fi

echo "large_board_bench executable was not produced at $runner" >&2
exit 1
