# cycletime (single feature: cycle time / lead time)

Dataset for cycle time (started to completed) and lead time (created to completed). It includes a
completed item without `start_at`, an item without a Sprint, and an active item to exercise filters.
All timestamps are fixed in 2026-06.

```bash
cargo run --manifest-path ../../../Cargo.toml -- cycletime                                  # aggregate all completed PBIs
cargo run --manifest-path ../../../Cargo.toml -- cycletime -S S-1                           # filter by Sprint (excludes T-6)
cargo run --manifest-path ../../../Cargo.toml -- cycletime --since 2026-06-05 --until 2026-06-10
cargo run --manifest-path ../../../Cargo.toml -- cycletime -j                               # JSON output
```
