# release-metadata (single feature: publication and SQLite compatibility checks)

This board demonstrates the release metadata contract and the SQLite schema v1-to-v2 recovery
guidance. The board data is repository-only and is excluded
from the published crate.

Run the board commands from this demo directory through the repository binary:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --plain
```

From the repository root, run the release gate:

```bash
./scripts/check-release-metadata.sh
mise run release-check
```

The checker compares `Cargo.toml`, every committed `Cargo.lock`, published
installation examples, the latest semantic-version tag, and the first dated
CHANGELOG heading. It also requires the complete compatibility guidance in
`docs/stability.md` before packaging or publishing.
