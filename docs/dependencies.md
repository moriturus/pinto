# Dependency decisions

## `rusqlite` (optional SQLite backend)

`rusqlite` implements the optional, non-default SQLite backend. The default
build keeps the file backend and does not compile this dependency; users opt in
with `--features sqlite` when a normalized local database is useful for their
Scrum board.

The dependency uses `bundled SQLite` so the opt-in build does not require a
system SQLite installation. That increases compile time and package size for
the opt-in path, but keeps the supported feature reproducible across hosts. The
file backend remains the lightweight default and the SQLite support is not a
removal target.

SQLite has an independent versioned schema. Any schema change must record its
dependency and persistence impact, provide an explicit migration or recovery
plan, and update the compatibility tests and guidance together.

## `serde_json`

`serde_json` remains an unconditional dependency. It powers both Automation
Plan parsing and the public `--json` output supported by multiple CLI
commands. Feature-gating it would either remove an advertised default CLI
capability or add configuration complexity without materially reducing the
default dependency graph.

## Selective Clippy pedantic lints

CI enables `must_use_candidate` and `redundant_closure_for_method_calls` in
addition to the default Clippy lint set. Both identify actionable mistakes or
mechanical simplifications and can be kept warning-free without obscuring the
implementation.

The full `clippy::pedantic` group is intentionally deferred because it mixes
these useful checks with high-volume, preference-driven warnings that would
require broad churn. In particular, `missing_errors_doc` currently reports on
most service functions; adopting it should be a focused documentation change
that explains each error contract rather than adding generic boilerplate.


## `termimad`

`termimad` renders PBI Markdown bodies for `pinto show` and the Kanban details
popup. It is the established, focused crate for terminal Markdown and
reuses the `crossterm` backend `ratatui` already pulls in, so it adds no new
terminal stack. Rolling our own Markdown renderer (headings, lists, tables,
code) would be far more code and more error-prone; a raw ANSI passthrough would
not strip syntax or wrap to width.

The TUI popup does not take a second dependency to bridge `termimad` into
`ratatui`: the shared renderer emits ANSI once and a small in-tree SGR parser
(`src/cli/markdown.rs`) converts each line into ratatui spans, so both display
paths share a single rendering. Redirected `show` output uses `termimad`'s
colourless skin so pipes and files stay clean text.

## `yaml-rust2` (test-only YAML parser)

`yaml-rust2` is a dev-only dependency used by the documentation tests in
`tests/docs.rs` to confirm that the CI workflows and `.github/dependabot.yml`
are structurally valid YAML. It replaces the archived, unmaintained
`serde_yaml` (0.9.34). The check only needs to parse a document, so the crate's
`YamlLoader::load_from_str` is sufficient and pulls in no serde surface. Because
the dependency is confined to `[dev-dependencies]`, it never ships in the
released binary.

## Automated dependency and Actions maintenance

Dependency and GitHub Actions updates are proposed automatically and stay
reviewable in Git history rather than drifting untracked.

- **Pinned Actions.** Every `uses:` reference in `.github/workflows/` is pinned
  to a full commit SHA with a trailing human-readable version comment, for
  example `actions/checkout@<sha> # v7.0.1`. The SHA is the immutable thing that
  runs; the comment records which release it is. `dtolnay/rust-toolchain` infers
  its toolchain from the git ref, so its SHA-pinned steps name the toolchain
  explicitly with `with: toolchain: stable` (or `nightly`).
- **Scheduled proposals.** `.github/dependabot.yml` opens weekly pull requests
  for the `cargo` and `github-actions` ecosystems. For an Actions update
  Dependabot rewrites both the commit SHA and the version comment together, so
  the pin stays honest.
- **Duplicate policy.** `deny.toml` sets `multiple-versions = "deny"`. The
  currently approved duplicate crate versions are recorded under `[bans].skip`
  with a reason; any newly introduced, unrecorded duplicate fails
  `mise run deny`.

### Reviewing an automated update proposal

When a Dependabot pull request arrives, a maintainer:

1. Confirms an Actions bump still pins a 40-character commit SHA and that the
   trailing version comment matches the release the SHA resolves to.
2. Runs `mise run check` (test, lint, docs, fmt), then `mise run audit` and
   `mise run deny` to validate advisories, licenses, and the duplicate policy.
3. If `mise run deny` reports a new duplicate version, either resolves it or
   records a reviewed `[bans].skip` entry (with a reason) in `deny.toml`, then
   re-runs the gate. Unexplained duplicates are not merged.

## `tempfile`

`tempfile` is an unconditional dependency because editor-backed commands need
an owner-private temporary buffer with exclusive creation, collision retries,
Unix 0600 permissions, and cleanup on success, failure, or panic. Its
`NamedTempFile` primitive provides those guarantees for the blocking
`$EDITOR` boundary. `tokio::fs` provides asynchronous file operations but not
this complete temporary-file lifecycle, so rebuilding it locally would add
security-sensitive code without helping the synchronous editor process.
