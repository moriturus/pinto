# lightweight-defaults (single feature: plain-text defaults and opt-in SQLite)

This demo starts with the default file backend. File and Git backends keep the
board in human-readable Markdown/TOML records; SQLite is an optional,
non-default SQLite backend and is the explicit persistence exception.

Run the default path from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --plain
```

To exercise SQLite, copy this demo to a disposable directory before migrating:

```bash
cargo run --features sqlite --manifest-path ../../../Cargo.toml -- migrate --to sqlite
cargo run --features sqlite --manifest-path ../../../Cargo.toml -- list --long
cargo run --features sqlite --manifest-path ../../../Cargo.toml -- migrate --to file
```

The SQLite commands require the explicit `sqlite` feature. Migration changes
the selected board's persistence format, so keep a backup and consult
[`docs/stability.md`](../../../docs/stability.md) before changing a real board.
