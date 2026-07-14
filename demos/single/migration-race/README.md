# migration-race (single feature: locked backend selection during migration)

This board starts on the file backend with two PBIs. A migration holds the board lock while it
copies data and switches `config.toml`; a writer that was already waiting then opens the backend
selected by the new configuration.

Inspect the starting data and walk through a file-to-Git migration:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- migrate --to git
cargo run --manifest-path ../../../Cargo.toml -- add "Write after the migration target is selected"
cargo run --manifest-path ../../../Cargo.toml -- list --long
git log --oneline -- .pinto
```

The same board can be moved to SQLite when the optional feature is enabled:

```bash
cargo run --features sqlite --manifest-path ../../../Cargo.toml -- migrate --to sqlite
cargo run --features sqlite --manifest-path ../../../Cargo.toml -- list --long
```

The deterministic interleaving and the read-only/no-lock rule are covered by the service tests:

```bash
cargo test --manifest-path ../../../Cargo.toml --all-features service::tests
```
