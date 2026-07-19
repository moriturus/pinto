# Changelog

All notable changes to pinto are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and releases use [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
