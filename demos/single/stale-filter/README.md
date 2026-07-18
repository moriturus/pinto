# stale-filter (single feature: stale PBI filtering)

This board demonstrates the read-only `list --stale` filter. `T-1` and `T-3`
describe stale work, while `T-2` is the item that is freshest immediately
after the setup commands.

Run these commands from this directory through the repository binary:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long
sleep 1
cargo run --manifest-path ../../../Cargo.toml -- list --stale 1s
cargo run --manifest-path ../../../Cargo.toml -- list --stale 1s --status todo --json
cargo run --manifest-path ../../../Cargo.toml -- list --stale 1s --label backend
```

Durations use a positive integer followed by `s`, `m`, `h`, `d`, or `w`; for
example, `7d` means unchanged for at least seven days. The stale query only
reads the board, so checking `show T-1 --json` before and after the query
returns the same `updated` timestamp.
