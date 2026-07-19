# storage-contract (single feature: backend and Markdown persistence contracts)

This demo contains a Unicode PBI and Sprint with optional fields, timestamps,
and relationships. It is a small CLI fixture for checking that the file,
Git, and optional SQLite backends expose the same domain workflow.

Run the file-backed workflow from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --plain
cargo run --manifest-path ../../../Cargo.toml -- sprint list
```

To try the same board with SQLite, copy the demo to a disposable directory and
run the migration with the explicit feature:

```bash
cargo run --features sqlite --manifest-path ../../../Cargo.toml -- migrate --to sqlite
cargo run --features sqlite --manifest-path ../../../Cargo.toml -- list --long
cargo run --features sqlite --manifest-path ../../../Cargo.toml -- sprint list
```

The property tests for Markdown round trips live in the storage module, while
the shared backend workflow is covered by `tests/storage_contract.rs`.
