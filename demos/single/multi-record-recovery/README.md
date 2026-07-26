# multi-record-recovery (single feature: recoverable multi-record mutations)

This demo is a small board for trying the operation-level recovery contract
used by `split` and `import --force`. Both commands prepare all records before
the operation is committed. A record-write failure restores the board to its
pre-operation state, so the command can be retried.

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --json
cargo run --manifest-path ../../../Cargo.toml -- split T-1 "First slice" "Second slice"
cargo run --manifest-path ../../../Cargo.toml -- export --json > snapshot.json
cargo run --manifest-path ../../../Cargo.toml -- import --force snapshot.json
```

File and SQLite restore the operation snapshot automatically when a write fails.
Git does the same before the final commit; if Git itself rejects that commit,
run `git status --short`, fix the hook or repository problem, and retry or
commit the complete durable change manually. These failure paths exit with
status 2 for an internal persistence failure, while input refusal without
`--force` exits with status 1.
If automatic restoration itself fails, the error keeps a recovery snapshot path;
preserve `.pinto/.lock`, restore that snapshot, inspect the board, and retry.
