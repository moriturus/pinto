# sprint-goal (single feature: Sprint Goal outcomes)

This dataset demonstrates the explicit boolean Sprint Goal result. `S-1` is
achieved, `S-2` is not achieved, and `S-3` has not been evaluated. `S-4` has a
blank Goal and is also excluded from the achievement-rate denominator.

Inspect the per-Sprint results and the aggregate rate with:

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint goal
cargo run --manifest-path ../../../Cargo.toml -- sprint goal --json
cargo run --manifest-path ../../../Cargo.toml -- sprint list --json
```

The report is 50.0%: one of the two non-blank, evaluated Goals was achieved.
Change or clear a result with `sprint edit S-1 --goal-achieved false` or
`sprint edit S-1 --clear-goal-achieved`.
