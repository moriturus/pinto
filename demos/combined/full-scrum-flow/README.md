# full-scrum-flow (combined: one-sprint scenario)

This combined demo places epics, parent/child links, dependencies, labels, WIP limits, and every
status (`todo`, `in-progress`, `review`, `done`) in one active Sprint, `S-1`. The fixed dataset is
dated 2026-06.

- Parent/child: the seeded dataset contains one epic with four children.
- Dependencies: four unresolved links form a shared-blocker graph with two dependent branches.
- WIP: `in-progress = 3` / `review = 2`.

```bash
cargo run --manifest-path ../../../Cargo.toml -- board -S S-1     # board with dependencies, status, and WIP
cargo run --manifest-path ../../../Cargo.toml -- show T-1         # list the epic's children
cargo run --manifest-path ../../../Cargo.toml -- show T-6         # PBI blocked by an unresolved dependency
cargo run --manifest-path ../../../Cargo.toml -- cycletime -S S-1 # cycle time for completed PBIs
cargo run --manifest-path ../../../Cargo.toml -- list -L feature  # filter by label
```
