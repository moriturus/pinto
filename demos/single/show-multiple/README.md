# show-multiple (single feature: multiple PBI details)

Dataset for displaying several PBIs in one command. The commands preserve the requested
order, separate human-readable details with a boundary, and return an array for JSON output.

```bash
cargo run --manifest-path ../../../Cargo.toml -- show T-2 T-1
cargo run --manifest-path ../../../Cargo.toml -- show T-2 T-1 --plain
cargo run --manifest-path ../../../Cargo.toml -- show T-2 T-1 --json
```
