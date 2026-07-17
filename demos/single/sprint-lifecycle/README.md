# sprint-lifecycle (single feature: Sprint management)

Dataset for the Sprint lifecycle. It contains `S-1` and `S-2` (closed) plus `S-3` (planned).
`S-2` was closed with `--rollover S-3`: unfinished `T-3` (5 points) and `T-5` (3 points)
moved to `S-3`, while `S-2` retained a close-time spillover snapshot of 8 points in 2 items.

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint list --json    # inspect close time and spillover snapshot
cargo run --manifest-path ../../../Cargo.toml -- board -S S-3          # rolled-over work is now in S-3
cargo run --manifest-path ../../../Cargo.toml -- sprint velocity -n 3  # S-2: 0 velocity, 8 spillover points
```

The persisted result was produced through the normal CLI with:

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint close S-2 --rollover S-3
```

The velocity average and change use completed points only; the 8 spillover points are displayed
separately and are not added to either calculation.
