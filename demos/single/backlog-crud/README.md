# backlog-crud (single feature: add / list / show / edit / remove)

Dataset for basic backlog operations. It contains five PBIs, including one archived item.

```bash
cargo run --manifest-path ../../../Cargo.toml -- list                 # list items
cargo run --manifest-path ../../../Cargo.toml -- list -l              # stable Scrum overview columns
cargo run --manifest-path ../../../Cargo.toml -- list -l --label      # include labels without filtering
cargo run --manifest-path ../../../Cargo.toml -- list -l --sprint     # include Sprint without filtering
cargo run --manifest-path ../../../Cargo.toml -- list -s todo         # filter by status
cargo run --manifest-path ../../../Cargo.toml -- list -L auth         # filter by label
cargo run --manifest-path ../../../Cargo.toml -- show T-1             # show details
cargo run --manifest-path ../../../Cargo.toml -- add "New PBI" -p 3 -l feature -b "Acceptance criteria"
cargo run --manifest-path ../../../Cargo.toml -- edit T-2 -a bob -p 8 # update assignee and points
cargo run --manifest-path ../../../Cargo.toml -- remove T-4           # archive to .pinto/archive/
```
