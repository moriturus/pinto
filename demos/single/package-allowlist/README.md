# package-allowlist (single feature: release package verification)

This board demonstrates the allowlisted crate contents, the committed package
file-list baseline used by the release checks.
The demo data is repository-only and is intentionally excluded from the crate.

Run commands from this demo directory through the repository binary:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --plain
```

From the repository root, run the corresponding verification commands:

```bash
ALLOW_DIRTY=1 ./scripts/verify-package.sh
cargo install --path . --locked --root "$PWD/.tmp/pinto"
```

The verifier runs `cargo package --all-features --locked`, compares the
archive against `release/package-files.txt`, and tests the extracted packaged
crate.
