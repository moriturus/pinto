# Closed Sprint assignment demo

This demo shows the Sprint assignment state rule: PBIs can be assigned while a
Sprint is `planned` or `active`, but a new assignment to a `closed` Sprint is a
user error. An assignment made before closing remains available for cleanup,
so `sprint unassign` continues to work after the Sprint is closed.

Inspect the final dataset from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint list --json
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --json
cargo run --manifest-path ../../../Cargo.toml -- show T-2 --json
cargo run --manifest-path ../../../Cargo.toml -- list --long
```

The reproducible workflow is:

```bash
cargo run --manifest-path ../../../Cargo.toml -- init
cargo run --manifest-path ../../../Cargo.toml -- sprint new S-1 "Sprint One" --goal "Ship the sprint"
cargo run --manifest-path ../../../Cargo.toml -- add "Assigned before close"
cargo run --manifest-path ../../../Cargo.toml -- add "Rejected after close"
cargo run --manifest-path ../../../Cargo.toml -- sprint start S-1
cargo run --manifest-path ../../../Cargo.toml -- sprint add S-1 T-1
cargo run --manifest-path ../../../Cargo.toml -- sprint close S-1
cargo run --manifest-path ../../../Cargo.toml -- sprint add S-1 T-2  # exits 1 with a repair hint
cargo run --manifest-path ../../../Cargo.toml -- sprint unassign S-1 T-1 # allowed after close
```

The checked-in state stops after the failed `sprint add`, so one seeded PBI records the
assignment made before closure and the other remains unassigned. Run the final
`sprint unassign` command to exercise the correction path.
