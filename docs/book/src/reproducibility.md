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
release metadata, and the release build/package tasks to the quality gate. The
release metadata task checks package versions in all committed lockfiles,
published installation examples, the latest release tag, and the CHANGELOG.
It also requires the [SQLite schema v1 to v2 compatibility guidance](../../stability.md)
to remain complete.

## Allowlisted package contents

The crate manifest uses root-anchored `package.include` entries for the
manifest, source, locale resources, README, license, and the rank benchmark
example. This allowlisted package excludes repository-only data such as
`.pinto`, demos, tests, docs, and CI metadata.

Run `./scripts/verify-package.sh` or `mise run release-package` to run
`cargo package --all-features --locked`, compare non-source package paths with
the committed package file list in `release/package-files.txt`, and verify every current `src/**` file
is present in the archive, and run tests against the extracted packaged crate.
The recursive source include is checked directly, so adding a Rust module does
not create a stale snapshot failure; update the baseline when a deliberate
non-source package path changes. CI also runs `cargo install --path . --locked`
from the clean checkout as the source-install check.

## Publishing a release

For each release, update the package version in `Cargo.toml` and both
committed lockfiles, move the relevant entries from `[Unreleased]` into a dated
`CHANGELOG.md` heading, and update the published-version installation examples. For a breaking change
while pinto remains in the `0.x` series, increment the minor version as the
`0.2.0` CLI rename demonstrates. Before publishing, run the complete local
release gate and verify the package without uploading it:

```bash
mise run release-check
cargo publish --dry-run --all-features --locked
```

The release gate must pass before a public release. A release is not ready
while the package version, lockfiles, installation examples, CHANGELOG heading,
and release tag disagree, or while the SQLite compatibility guidance is
incomplete. Keep the next work items under the undated `[Unreleased]` heading
until the release commit is tagged.

After the release commit has passed CI and has been fast-forwarded to `main`,
create the repository's version tag and push it together with `main`. Publish
the same locked package to crates.io only after the tag points at that commit:

```bash
git tag 0.2.0
git push origin main 0.2.0
mise run release-publish
```
