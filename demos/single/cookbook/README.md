# Cookbook pipelines

This demo accompanies the [Cookbook](../../../docs/book/src/cookbook.md)
chapter of the mdBook guide. The checked-in board matches the chapter's seed
data, so every text-stream recipe can be replayed here without setting up a
temporary directory.

The board was built with these commands (already applied — rerun them only
when recreating the demo from scratch):

```bash
cargo run --manifest-path ../../../Cargo.toml -- init
cargo run --manifest-path ../../../Cargo.toml -- add "Design the login form" --points 3 --label ui --label auth
cargo run --manifest-path ../../../Cargo.toml -- add "Implement the login API" --points 5 --label api --label auth
cargo run --manifest-path ../../../Cargo.toml -- add "Write onboarding docs" --points 2 --label docs
cargo run --manifest-path ../../../Cargo.toml -- add "Fix the session timeout bug" --points 1 --label bug --label auth
cargo run --manifest-path ../../../Cargo.toml -- add "Refactor the storage layer" --points 8 --label refactor
cargo run --manifest-path ../../../Cargo.toml -- move T-1 in-progress
cargo run --manifest-path ../../../Cargo.toml -- move T-2 review
cargo run --manifest-path ../../../Cargo.toml -- sprint new S-1 "Sprint 1" --goal "Ship the login flow" --start 2026-07-13 --end 2026-07-27
cargo run --manifest-path ../../../Cargo.toml -- sprint add S-1 --status todo --limit 2
cargo run --manifest-path ../../../Cargo.toml -- sprint start S-1
```

Try the pipeline recipes from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --status todo | cut -d' ' -f1
cargo run --manifest-path ../../../Cargo.toml -- list | tr -s ' ' | cut -d' ' -f2 | sort | uniq -c
cargo run --manifest-path ../../../Cargo.toml -- list --sprint S-1 | cut -d' ' -f1 | paste -sd' ' -
```

The first prints the IDs of the remaining `todo` PBIs, the second counts PBIs
per status, and the third folds the sprint assignments onto one line. See the
chapter for the full set of recipes, including `grep`, `head`, `tail`, `sed`,
`join`, and feeding pipeline output back into `pinto move`.
