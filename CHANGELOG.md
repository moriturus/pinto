# Changelog

All notable changes to pinto are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and releases use [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.3] - 2026-07-31

This patch release adds Sprint records and strengthens board integrity,
interchange, and cross-platform CLI reliability without breaking the existing
CLI contract.

### Added

- Added Sprint Goal achievement tracking and Sprint Goal reports.
- Added separate Sprint Retro and Review records with linked action PBIs and
  Sprint context.
- Preserved Sprint child records across board export/import and backend
  migrations.

### Fixed

- Strengthened Sprint deletion, goal-outcome, and record-integrity checks.
- Prevented Windows CLI stack overflows during completion, shell, and
  automation processing.
- Isolated the doctor check's Windows executable from parallel integration
  tests.

## [0.4.2] - 2026-07-28

This patch release adds external subcommands and improves automation and
release workflows without changing the board data format or breaking the
existing CLI contract.

### Added

- Added external subcommand binaries with nested command discovery, argument
  forwarding, exit-code propagation, and versioned contract environment
  variables.
- Added escaped literal placeholders to sequential `pinto automate` plans.
- Added versioned Book deployments and a tag-driven GitHub Release workflow
  that derives release notes from `CHANGELOG.md`.

### Fixed

- Fixed CI and dependency-policy validation for `syn` 3 dependency updates.
- Improved Windows compatibility for external command dispatch.

## [0.4.1] - 2026-07-28

This patch release improves automation, deep-board safety, and multi-record
recovery without changing the board data format or breaking the existing CLI
contract.

### Added

- Added output placeholders to sequential `pinto automate` plans, allowing
  later commands to consume IDs produced by earlier `add` and `split` commands
  through references such as `@command[0].created_ids[0]`.
- Added operation-level recovery for `split` and `import --force` across the
  File, Git, and SQLite backends, with restoration or transaction guarantees
  and actionable Git commit-failure guidance.
- Added large-board performance regression checks with reproducible benchmark
  reports and CI coverage for the smoke and scheduled benchmark paths.

### Fixed

- Fixed deep parent/child ordering, Kanban layout, point aggregation, and
  dependency-cycle inspection to avoid native stack overflows on deeply nested
  boards.
- Fixed the Kanban help overlay so it lists the existing `split` action.

### Documentation

- Documented public Rust API error contracts and the pre-tag release
  verification workflow.

## [0.4.0] - 2026-07-25

This minor release adds PBI splitting, letting you derive new PBIs from an
existing item without changing the board data format or breaking the
existing CLI contract.

### Added

- Added the `pinto split <source> <title>...` subcommand (alias `spl`) to
  derive one or more new PBIs from an existing item while keeping the
  source. A relationship flag optionally makes the source a parent
  (`--child`) of, or a dependency (`--dependency`) on, each new PBI. A body
  flag controls the new PBI's body: copy the source (default), `--empty`,
  `--template <name>`, or `--body <text>`.
- Added the same split operation to the Kanban board under the `s` key,
  driven by a stepped form (title, relationship, body).

## [0.3.3] - 2026-07-22

This patch release improves Kanban modified-key handling across terminal event
encodings without changing the board data format or CLI contract.

### Fixed

- Fixed configurable Kanban modified-key bindings across crossterm terminal
  event encodings, including the documented `Ctrl+?` regular-expression
  search shortcut.

### Documentation

- Added team-scale best-practice guidance to the book: individual development without Scrum
  features, Scrum features for small teams, and the Git backend in a dedicated repository for
  larger teams.

## [0.3.2] - 2026-07-22

This patch release adds Git-backed undo and board import/recovery workflows
without changing existing board file formats or the default file backend.

### Added

- Added `pinto undo` to undo the most recent completed mutation on the Git
  backend.
- Added `pinto import <SOURCE>` to restore a board from an `export --json`
  snapshot, including standard input and an explicit `--force` option
  for replacing a populated board.
- Added a runbook for resolving Git merge conflicts in shared pinto boards.

### Changed

- `pinto doctor --fix` can repair unambiguous duplicate PBI IDs while
  preserving the issued-ID history.
- Localized all pinto-authored CLI argument help text in English and Japanese.
- Improved large-board inspection, import, and write scaling with bounded
  asynchronous reads and batched file-backend writes.

### Fixed

- Prevented scope-internal rank collisions when moving PBIs.

### Documentation

- Added reproducible large-board benchmarks and recorded the decision to keep
  complete-board validation for single-item reads.

## [0.3.1] - 2026-07-20

This patch release reorganizes internal implementation and test modules
without changing the CLI, public Rust API, or board data formats.

### Changed

- Split oversized CLI command, formatting, Kanban runtime, SQLite repository,
  service, and integration-test files into focused Rust modules.
- Preserved existing module paths and runtime behavior through parent-module
  re-exports.

## [0.3.0] - 2026-07-19

This minor 0.x release adds backlog discovery, recovery, diagnostics,
machine-readable workflows, and richer Sprint and Kanban reporting.

### Added

- Added per-user Kanban keybindings in `$XDG_CONFIG_HOME/pinto/config.toml`,
  keeping personal preferences out of shared `.pinto/config.toml` board state.
- Added `pinto export --json` for read-only snapshots containing active PBIs,
  Sprints, effective configuration, and the shared Definition of Done.
- Added `pinto automate --schema` to print the Draft 2020-12 schema for safe
  automation plans.
- Added `pinto next` to find ranked, unstarted PBIs whose dependencies are
  complete, with count, Sprint, and JSON options.
- Added `pinto doctor` for board-integrity diagnostics and conservative
  mechanical repairs with `--fix`.
- Added archived-PBI inspection and recovery through `--archived` and
  `pinto restore`.
- Added ancestor board discovery, with `--dir` and `PINTO_DIR` overrides for
  scripts and nested working directories.
- Added stale-PBI filtering, exact assignee filters for `list` and `board`,
  and Sprint/label filters for the Kanban view.
- Added Markdown Acceptance Criteria progress in item details and long-form
  list/board output, plus a warning when incomplete criteria reach the done
  column.
- Added Sprint close handling for unfinished work (`--rollover` and
  `--release`), close-time spillover snapshots, and non-blocking load warnings
  based on capacity and recent velocity.
- Added support for supplying multiple label values after one `--label`
  option; repeating the option remains supported.

### Changed

- Moved personal Kanban keybindings out of shared `.pinto/config.toml` and
  into `$XDG_CONFIG_HOME/pinto/config.toml`. A newer binary rejects the legacy
  shared `[tui.key_bindings]` table; copy those preferences to the user file
  before upgrading. After that table is removed, older binaries can read the
  board configuration unless another newly added board key is present.

### Documentation

- Added a reproducible local CI guide for `act` and expanded the CLI, data
  format, JSON contract, and workflow documentation for the new commands.
- Documented the compatibility boundary between strict board configuration,
  Markdown board data, versioned SQLite storage, and JSON output. Releases that
  add board configuration keys must state older-binary readability and provide
  downgrade guidance in the release notes.

## [0.2.0] - 2026-07-17

This minor 0.x release makes the Git commit-link command name match its
write behavior. It is a breaking CLI and public Rust API change under the
0.x versioning policy.

### Changed

- Renamed `pinto link scan` to `pinto link sync`. The command synchronizes
  PBI commit links by matching PBI IDs in Git commit messages, and the old
  command name is no longer accepted.
- Renamed the public Rust service API `scan_commits` / `ScanOutcome` to
  `sync_commits` / `SyncOutcome` so the API terminology matches the command.
- Updated CLI help, English and Japanese localization, README, the book,
  workflow skill guidance, and the Git-link synchronization demo.

## [0.1.1] - 2026-07-16

This patch release improves cross-platform reliability without changing the
board file format or the existing CLI contract.

### Fixed

- Inline JSON automation plans are no longer rejected as invalid filesystem
  paths on Windows. Malformed, missing, and directory sources now return
  actionable source errors.
- Windows board-lock identity checks use stable Win32 handle APIs, keeping lock
  cleanup safe when the same file is opened through different handles.

### Changed

- CI now validates pushes to `develop` with pinned toolchains and a Cobertura-
  based coverage gate, stabilizing macOS and Windows quality checks.
- The installation and reproducible-release documentation now describes the
  published 0.1.1 package and release verification flow.

## [0.1.0] - 2026-07-15

This is the initial 0.x release. pinto follows Semantic Versioning, but
backward compatibility for the CLI, data format, and public Rust API is not
guaranteed across 0.x minor releases; breaking changes are documented in the
release notes.

### Added

- The initial local-first Scrum backlog and Kanban workflow: initialize boards,
  manage Product Backlog Items, with support for labels, points, parent-child
  relationships, dependencies, and a shared Definition of Done.
- Sprint planning and reporting: create, edit, start, close, delete, assign,
  and unassign Sprints, with capacity, burndown, velocity, and cycle-time
  reports.
- Terminal interfaces for non-interactive commands, board and Kanban views,
  an interactive shell, detailed output, filtering, and machine-readable JSON.
- Plain-text Markdown/TOML board storage with fractional ranks and explicit
  rebalancing, plus Git and optional SQLite backends with migration support.
- Validated structured automation plans with safe previews and machine-readable
  results.
- Git commit linking and scanning, Product Backlog Item and Sprint templates,
  and Fluent-based English and Japanese localization for CLI, TUI, help, and
  error messages.

### Changed

- The first release establishes `.pinto/` as the board data directory and
  requires the explicit configuration schema; unknown keys and missing
  required sections are rejected instead of silently using legacy defaults.
- Write operations use atomic file replacement and board-level advisory
  locking, while Git-backed writes commit complete service operations and
  preserve unrelated working-tree changes.
- Machine-readable JSON provides stable command results, while pinto-generated
  diagnostics follow the selected locale.

## Versioning policy

pinto remains in the `0.x.y` development series. During this period, a
breaking CLI, data-format, or public Rust API change increments the minor
version (`0.x.0`); backward-compatible features increment the patch version
(`0.x.y`). Patch releases may also contain backward-compatible bug fixes.

Once version 1.0.0 is released, Semantic Versioning's normal major-version
rules apply. Every release must move relevant Unreleased entries into a dated
version heading.
