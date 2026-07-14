# Sprint edit and remove demo

This demo covers the recovery path for a planned Sprint created without a
goal. `sprint edit` updates its title, goal, and planned period, after which
`sprint start` can move it to active. `sprint remove` removes a Sprint while
keeping its PBIs and clearing their `sprint` assignment.

Run these inspection commands from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint list --json
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-2 --json
```

The final dataset contains an active Sprint with its goal and period edited,
plus a PBI retained in the backlog with no Sprint after the other Sprint was
deleted. The workflow used to create those states is:

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint new S-1 "Planning without a goal"
cargo run --manifest-path ../../../Cargo.toml -- add "Work for the edited sprint"
cargo run --manifest-path ../../../Cargo.toml -- sprint add S-1 T-1
cargo run --manifest-path ../../../Cargo.toml -- sprint edit S-1 --goal "Ship the first release" --start 2026-07-06 --end 2026-07-10
cargo run --manifest-path ../../../Cargo.toml -- sprint start S-1
cargo run --manifest-path ../../../Cargo.toml -- sprint new S-2 "Discarded sprint"
cargo run --manifest-path ../../../Cargo.toml -- add "Unassigned after sprint deletion"
cargo run --manifest-path ../../../Cargo.toml -- sprint add S-2 T-2
cargo run --manifest-path ../../../Cargo.toml -- sprint remove S-2
```

Removing a Sprint never deletes its PBIs; it releases every assigned PBI back
to the backlog and commits the complete operation together for Git-backed
boards.
