# split (single feature: PBI splitting)

Dataset for splitting an existing PBI into new PBIs. `T-1` (an epic) has been
split three ways: two children copy its body under a parent-child relationship,
one spike it depends on carries an explicit body, and one independent follow-up
starts with an empty body.

```bash
cargo run --manifest-path ../../../Cargo.toml -- show T-1                 # children T-2, T-3 and dependency T-4
cargo run --manifest-path ../../../Cargo.toml -- split T-1 "Cart summary page" "Payment step" --child   # source becomes the parent, bodies copied
cargo run --manifest-path ../../../Cargo.toml -- split T-1 "Payment gateway spike" --dependency --body "Evaluate two payment providers."
cargo run --manifest-path ../../../Cargo.toml -- split T-1 "Checkout analytics" --empty                 # independent PBI, empty body
```

`split <SOURCE> <TITLE>...` creates one PBI per title. The relationship flags
`--child` (the source parents each new PBI) and `--dependency` (the source
depends on each new PBI) are mutually exclusive. The body flags `--body`,
`--template`, and `--empty` are mutually exclusive; omitting them copies the
source body. The same operation is available inside the Kanban board with the
`s` key.
