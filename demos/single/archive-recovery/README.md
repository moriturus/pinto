# archive-recovery (single feature: archived PBI recovery)

Dataset for inspecting archived PBIs without mixing them into the active backlog,
then restoring one without changing its ID or content.

Run these commands from this directory through the repository binary:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list
cargo run --manifest-path ../../../Cargo.toml -- list --archived --json
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --archived
cargo run --manifest-path ../../../Cargo.toml -- restore T-1
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --json
```

`T-1` starts in `.pinto/archive/`, while `T-2` remains active. The normal
`list`, `board`, and `show` views exclude `T-1` until the restore command moves
it back to `.pinto/tasks/`. Restoring an ID that is already active exits with an
error and does not overwrite either record.
