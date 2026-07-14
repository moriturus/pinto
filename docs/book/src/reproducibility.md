# Reproducible builds and releases

The repository commits `Cargo.lock` and treats it as part of the source and
release contract. Cargo commands that build, test, document, package, or
install pinto must use `--locked`; an intentional dependency update is made
with `cargo update`, followed by review of the lockfile diff.

## Toolchain roles

Development and release commands use Rust 1.97.0, pinned in `mise.toml`.
`Cargo.toml` continues to declare Rust 1.89 as the minimum supported version.
CI keeps the responsibilities separate:

| Job | Toolchain | Scope |
| --- | --- | --- |
| `msrv` | Rust 1.89.0 | Default and all-feature build/test compatibility |
| `check` | Pinned Rust 1.97.0 | Full `mise run check` quality gate on each primary OS |
| `current-stable` | Latest stable channel | Forward-compatibility test suite with all features |
| `release` | Pinned Rust 1.97.0 | Release build, package, and source-install verification |

The all-feature MSRV checks and the pinned quality gate intentionally cover
different support contracts. The latest-stable job does only the forward
compatibility probe, so a moving toolchain does not define release artifacts.

## Clean-checkout verification

From a clean checkout, install the pinned tools and run the same gates used by
CI:

```bash
mise install
mise run check
cargo build --release --all-features --locked
cargo package --all-features --locked
cargo install --path . --locked --root "$PWD/.tmp/pinto"
```

`mise run release-check` adds coverage, dependency audit, dependency policy,
and the release build/package tasks to the quality gate.

## Allowlisted package contents

The crate manifest uses root-anchored `package.include` entries for the
manifest, source, locale resources, README, license, and the rank benchmark
example. This allowlisted package excludes repository-only data such as
`.pinto`, demos, tests, docs, and CI metadata.

Run `./scripts/verify-package.sh` or `mise run release-package` to run
`cargo package --all-features --locked`, compare the package file list with the
committed `release/package-files.txt`, and run tests against the extracted
packaged crate. A deliberate runtime-file addition must update the allowlist
and its file-list baseline together. CI also runs `cargo install --path . --locked`
from the clean checkout as the source-install check.
