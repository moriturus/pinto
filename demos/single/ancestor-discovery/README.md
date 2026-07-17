# ancestor-discovery (single feature: board discovery)

This dataset demonstrates selecting the nearest ancestor `.pinto` board from a nested working
directory. The checked-in board is at the demo root, while `nested/src/` represents a repository
subdirectory where a command can be run.

Run these commands from this directory through the repository binary:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list
cargo run --manifest-path ../../../Cargo.toml -- --dir .pinto list
PINTO_DIR="$PWD" cargo run --manifest-path ../../../Cargo.toml -- list
```

The ancestor lookup also stops at a directory containing `.git`, after checking that directory.
Use `--dir PATH` or `PINTO_DIR` when a script must select a board explicitly.

To try nested discovery without changing the fixture, run the same binary from `nested/src/`:

```bash
(cd nested/src && cargo run --manifest-path ../../../../../Cargo.toml -- list)
```
