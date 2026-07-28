# Reproducible builds and releases

The repository commits `Cargo.lock` and treats it as part of the source and
release contract. Cargo commands that build, test, document, package, or
install pinto must use `--locked`; an intentional dependency update is made
with `cargo update`, followed by review of the lockfile diff.

## Toolchain roles

Development and release commands use Rust 1.97.0, pinned in `mise.toml`.
`Cargo.toml` continues to declare Rust 1.89 as the minimum supported version.
CI keeps the responsibilities separate:

| Job | Workflow | Toolchain | Scope |
| --- | --- | --- | --- |
| `msrv` | `ci.yml` | Rust 1.89.0 | Default and all-feature build/test compatibility |
| `check` | `ci.yml` | Pinned Rust 1.97.0 | Full `mise run check` quality gate on each primary OS |
| `current-stable` | `ci.yml` | Latest stable channel | Forward-compatibility test suite with all features |
| `release` | `release.yml` | Pinned Rust 1.97.0 | Release build, package, source-install, and GitHub Release creation |

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

## Release and security responsibilities

The release maintainer owns the version, lockfiles, changelog, package,
release tag, and publication checks described below. The security maintainer
owns private vulnerability intake, triage, reporter communication, and
coordinated disclosure; see the
[security policy](https://github.com/moriturus/pinto/blob/main/SECURITY.md). A
primary maintainer may hold both roles, but the responsibilities and review
record remain explicit.

Release-related or security-related changes include a documented risk
assessment, responsible maintainer, strongest applicable release and security
checks, and follow-up actions. If one maintainer holds both roles, record that
ownership explicitly. This documented fallback preserves traceability and does
not waive the normal verification expectation.

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

Choose the next version once and derive every command below from the manifest so
the procedure never embeds a stale published version. After bumping the version
in `Cargo.toml`, export it from `cargo pkgid`:

```bash
VERSION="$(cargo pkgid | sed 's/.*[@#]//')"
```

For each release, update the package version in `Cargo.toml` and both committed
lockfiles, move the relevant entries from `[Unreleased]` into a dated
`CHANGELOG.md` heading, and update the published-version installation examples to
match `$VERSION`. For a breaking change while pinto remains in the `0.x` series,
increment the minor version, as the earlier CLI rename demonstrates.

### Pre-tag verification

Before creating the tag, confirm the bumped tree is internally consistent and the
tag is still available. These checks require the package version in `Cargo.toml`,
both committed lockfiles, the dated `CHANGELOG.md` entry, and the installation
examples to agree on `$VERSION`, and that the `$VERSION` tag does not already
exist:

```bash
test "$(cargo pkgid | sed 's/.*[@#]//')" = "$VERSION"                    # Cargo.toml package version
for lock in $(git ls-files '*Cargo.lock'); do grep -Fq "version = \"$VERSION\"" "$lock" || echo "missing $VERSION in $lock"; done
grep -Fq "## [$VERSION]" CHANGELOG.md                                    # dated changelog entry
grep -Fq "cargo install pinto-cli --version $VERSION" README.md docs/book/src/installation.md
git tag --list "$VERSION" | grep -qx "$VERSION" \
  && { echo "tag $VERSION already exists"; false; } \
  || echo "tag $VERSION is available"
```

Once these pass, create the tag on the release commit so the release-metadata
gate — which treats the tag as the publication source of truth — sees a
consistent tree, then run the complete local gate and verify the package without
uploading it:

```bash
git tag "$VERSION"
mise run release-check
cargo publish --dry-run --all-features --locked
```

The release gate must pass before a public release. A release is not ready while
the package version, lockfiles, installation examples, CHANGELOG heading, and
release tag disagree, or while the SQLite compatibility guidance is incomplete.
Keep the next work items under the undated `[Unreleased]` heading until the
release commit is tagged.

The tag-triggered `release.yml` workflow extracts the matching dated section from
`CHANGELOG.md` with `scripts/extract-release-notes.sh` and runs
`gh release create` with the GitHub-provided token.
The workflow therefore creates the GitHub Release and its notes automatically
after the build and package checks pass; no manual release-entry step is needed.

## Published Book destinations

The Pages workflow builds one artifact from the stable `main` ref, the current
`develop` ref, and every semantic-version tag before deploying it.
This keeps the routes available together:

- `/pinto/` redirects to `/pinto/latest/`, which contains the stable `main` Book.
- `/pinto/develop/` contains the development Book.
- `/pinto/X.Y.Z/` contains the Book for the `X.Y.Z` release tag.

The root is a small static redirect entry point, while `/latest/` is built
directly from `main`; this avoids duplicate stable files and prevents the
latest route from drifting. Historical version routes are rebuilt into the
same artifact, so publishing a newer release does not remove older
documentation.

After the release commit has passed CI and has been fast-forwarded to `main`,
push the tag together with `main`. Publish the same locked package to crates.io
only after the tag points at that commit:

```bash
git push origin main "$VERSION"
mise run release-publish
```
