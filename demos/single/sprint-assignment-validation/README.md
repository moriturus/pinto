# Sprint assignment validation demo

This demo shows that every item-assignment entry point accepts only a valid,
existing Sprint. Validation happens while the board write lock is held, so a
failed assignment does not create an item or change an existing one.

Run the commands from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- init
cargo run --manifest-path ../../../Cargo.toml -- sprint new S-1 "Sprint One" --goal "Keep assignments valid"
cargo run --manifest-path ../../../Cargo.toml -- add "Created in an existing sprint" --sprint S-1 --points 3
cargo run --manifest-path ../../../Cargo.toml -- add "Assigned through edit"
cargo run --manifest-path ../../../Cargo.toml -- edit T-2 --sprint S-1
cargo run --manifest-path ../../../Cargo.toml -- sprint unassign S-1 T-2
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --plain
```

These commands intentionally fail with actionable user errors and leave the
board unchanged:

```bash
cargo run --manifest-path ../../../Cargo.toml -- add "Missing Sprint" --sprint S-404
cargo run --manifest-path ../../../Cargo.toml -- edit T-2 --sprint "S 1"
cargo run --manifest-path ../../../Cargo.toml -- sprint add "S 1" T-2
```
