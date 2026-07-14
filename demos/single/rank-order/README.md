# rank-order (single feature: reorder / scoped rebalance)

Dataset for backlog rank operations. Five items are arranged in the `todo` column.
`rebalance` generates short, evenly spaced ranks per `(status, parent)` sibling
scope; unrelated columns and parent groups are not rewritten.

```bash
cargo run --manifest-path ../../../Cargo.toml -- list                     # current order
cargo run --manifest-path ../../../Cargo.toml -- reorder T-5 --top        # move to the top
cargo run --manifest-path ../../../Cargo.toml -- reorder T-1 --bottom     # move to the bottom
cargo run --manifest-path ../../../Cargo.toml -- reorder T-3 --before T-2 # move before a PBI
cargo run --manifest-path ../../../Cargo.toml -- reorder T-4 --after T-1  # move after a PBI
cargo run --manifest-path ../../../Cargo.toml -- rebalance --dry-run      # preview oversized scopes
cargo run --manifest-path ../../../Cargo.toml -- rebalance                # rewrite only those scopes
```
