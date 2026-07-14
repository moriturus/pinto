# toolchain-reproducibility (single feature: reproducible builds and releases)

This board contains PBIs for checking the pinned development/release toolchain,
the committed Cargo lockfile, and the clean-checkout package path.

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --plain
cargo run --manifest-path ../../../Cargo.toml -- show T-2 --plain
cargo run --manifest-path ../../../Cargo.toml -- board
```

From the repository root, the corresponding verification commands are:

```bash
mise install
mise run check
cargo build --release --all-features --locked
cargo package --all-features --locked
cargo install --path . --locked --root "$PWD/.tmp/pinto"
```
