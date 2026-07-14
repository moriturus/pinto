# sprint-lifecycle (single feature: Sprint management)

Dataset for the Sprint lifecycle. It contains `S-1` (closed), `S-2` (active), and `S-3` (planned).

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint list           # list Sprints and statuses
cargo run --manifest-path ../../../Cargo.toml -- board -S S-2          # board for the active Sprint
cargo run --manifest-path ../../../Cargo.toml -- sprint new S-4 "Next Sprint" -s 2026-07-01 -e 2026-07-12
cargo run --manifest-path ../../../Cargo.toml -- sprint add S-3 T-4    # assign a PBI
cargo run --manifest-path ../../../Cargo.toml -- sprint unassign S-3 T-4 # unassign a PBI
cargo run --manifest-path ../../../Cargo.toml -- sprint start S-3      # planned → active
cargo run --manifest-path ../../../Cargo.toml -- sprint close S-2      # active → closed
```
