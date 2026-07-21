# doctor (single feature: board integrity checks)

This board demonstrates the integrity scan and the conservative repair mode.
The fixture starts healthy and contains two PBIs plus one Sprint.

Run commands from this demo directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- doctor
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --plain
```

`doctor` reports dangling references, relation cycles, duplicate IDs,
issued-ID history problems, invalid workflow states, rank anomalies, and
tasks/archive filename collisions. Every finding includes a path or PBI and
an explicit repair direction.

To demonstrate the safe repair path, rename one task and run the fixer:

```bash
mv .pinto/tasks/T-1.md .pinto/tasks/renamed.md
cargo run --manifest-path ../../../Cargo.toml -- doctor --fix
```

The fixer can restore an unambiguous filename, append an existing PBI ID to
`issued_ids`, and deterministically renumber duplicate PBI IDs while preserving
matching parent/dependency lineages. It does not choose which relationship,
status, rank, or storage collision should be removed. A rank anomaly names
`pinto rebalance` as its repair; see the [`rank-order`](../rank-order) demo for
that command.
