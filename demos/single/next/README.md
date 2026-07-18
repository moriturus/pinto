# next (single feature: actionable backlog candidates)

Dataset for `pinto next`. It contains blocked, ready, completed, and already-started PBIs across
two Sprints so the candidate rules are visible.

```bash
cargo run --manifest-path ../../../Cargo.toml -- next             # show the highest-ranked ready PBI
cargo run --manifest-path ../../../Cargo.toml -- next -n 2        # show two ready PBIs in rank order
cargo run --manifest-path ../../../Cargo.toml -- next --sprint S-1 # restrict candidates to S-1
cargo run --manifest-path ../../../Cargo.toml -- next --json      # emit the candidates as JSON
```

`T-1` is blocked by the active `T-2`. `T-3` depends on completed `T-4` and is the first actionable
candidate. `T-5` is another ready PBI in `S-2`, while `T-6` has already started.
