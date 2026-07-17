# Changelog

All notable changes to pinto are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and releases use [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
