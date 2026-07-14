# move-multiple (single feature: move multiple PBIs)

Dataset for transitioning several PBIs in one command. Like Unix `mv`, the last operand is the
destination column and every operand before it is a PBI to move. The board starts with four `todo`
PBIs, and `in-progress` has a WIP limit of 1 so the batch warning can be observed.

```bash
cargo run --manifest-path ../../../Cargo.toml -- list
cargo run --manifest-path ../../../Cargo.toml -- move T-1 T-2 review        # move both to review
cargo run --manifest-path ../../../Cargo.toml -- move T-3 T-4 in-progress   # over the WIP limit → warns once
cargo run --manifest-path ../../../Cargo.toml -- move T-404 T-1 done       # report the missing ID and continue
cargo run --manifest-path ../../../Cargo.toml -- move T-2 T-3 done --no-wip-check
```

The final operand is the destination status. When a requested ID is missing or the destination is
not a column, pinto reports the error and still moves every valid ID; the command exits with status
1 if at least one operand failed. The destination column's WIP limit is checked once after the batch,
and `--no-wip-check` suppresses that warning.
