# quality-validation (single feature: validation and smoke tests)

This board contains small PBIs with bodies that are useful when checking the
Markdown parser, public API examples, and the Kanban details popup.

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --plain
cargo run --manifest-path ../../../Cargo.toml -- board
cargo run --manifest-path ../../../Cargo.toml -- kanban
```

For the developer test layers, run these commands from the repository root:

```bash
cargo test --doc
cargo test --test cli
cargo check --manifest-path fuzz/Cargo.toml --bins
cargo fuzz run markdown_frontmatter_parse -- -max_total_time=300
```
