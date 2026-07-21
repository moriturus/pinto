# demos — feature coverage datasets

This directory contains ready-to-use local boards for trying pinto features. All data is plain
text (Markdown and TOML) and can be edited freely.

Run commands from each demo directory through the repository binary:

```bash
cargo run --manifest-path ../../../Cargo.toml -- <args>       # for example: ... -- board
```

## Layout

- [`single/`](single) — **single-feature** datasets for one command or feature at a time.
  - [`doctor`](single/doctor) — board integrity diagnostics and conservative safe repairs
  - [`automation-plan`](single/automation-plan) — safe structured plans, schema inspection, file input, dry-run, and JSON results
  - [`export`](single/export) — complete active-board JSON snapshots with configuration and shared DoD
  - [`backlog-crud`](single/backlog-crud) — add / list / show / edit / remove (archive)
  - [`archive-recovery`](single/archive-recovery) — list, inspect, and restore archived PBIs
  - [`ancestor-discovery`](single/ancestor-discovery) — discover the nearest ancestor board and use explicit directory overrides
  - [`cookbook`](single/cookbook) — seed board for the Cookbook chapter's Unix text-stream pipeline recipes
  - [`cli-test-layout`](single/cli-test-layout) — feature-oriented CLI integration-test fixtures
  - [`status-filter`](single/status-filter) — filter `list` and `board` by assignee, multiple statuses, and other filters
  - [`remove-multiple`](single/remove-multiple) — remove multiple PBIs, including partial failures and `--force`
  - [`move-multiple`](single/move-multiple) — move multiple PBIs at once (`mv`-style), batch WIP warning, partial failures
  - [`board-view`](single/board-view) — board columns, sorting, filtering, and truncation
  - [`acceptance-criteria`](single/acceptance-criteria) — Markdown checkbox progress, long-form columns, and completion warnings
  - [`kanban-keybindings`](single/kanban-keybindings) — configurable Kanban keys, modifiers, and aliases
  - [`tui-lifecycle`](single/tui-lifecycle) — terminal restore, resize, quit, and editor handoff
  - [`editor-buffer-security`](single/editor-buffer-security) — secure temporary editor buffers and RAII cleanup
  - [`kanban-card-meta`](single/kanban-card-meta) — story points (◆) and assignee (@) shown on Kanban cards
  - [`markdown-rendering`](single/markdown-rendering) — Markdown rendering in `show` and the Kanban details popup (with `--plain` opt-out and safe fallback)
  - [`rank-order`](single/rank-order) — reorder (before/after/top/bottom) and rebalance
  - [`wip-limit`](single/wip-limit) — WIP limit warnings and `--no-wip-check`
  - [`dependencies`](single/dependencies) — dependencies and board markers
  - [`i18n`](single/i18n) — English/Japanese CLI messages, `--help` argument descriptions, and diagnostic policy
  - [`i18n-localizer-cache`](single/i18n-localizer-cache) — repeated localized list, show, board, and Kanban rendering
  - [`parse-error-classification`](single/parse-error-classification) — hand-edited config/frontmatter diagnostics and exit-code classification
  - [`config-validation`](single/config-validation) — strict config schema and workflow semantic validation
  - [`file-id-integrity`](single/file-id-integrity) — filename/frontmatter IDs, duplicate detection, archive validation, and safe next-ID allocation
  - [`git-boundary`](single/git-boundary) — Git commit boundaries, staged-change isolation, and recovery
  - [`git-link-sync`](single/git-link-sync) — synchronize Git commit links from PBI IDs in commit messages
  - [`merge-conflict`](single/merge-conflict) — resolve parallel-clone ID collisions in `issued_ids` and task files, then verify with doctor
  - [`undo`](single/undo) — revert the most recent completed board mutation on the Git backend
  - [`migration-race`](single/migration-race) — backend migration lock ordering and writer visibility
  - [`quality-validation`](single/quality-validation) — Markdown parser, public API, and Kanban smoke-test data
  - [`toolchain-reproducibility`](single/toolchain-reproducibility) — pinned toolchain, locked installs, and release package checks
  - [`package-allowlist`](single/package-allowlist) — allowlisted crate contents and packaged-crate verification
  - [`lightweight-defaults`](single/lightweight-defaults) — plain-text defaults and opt-in SQLite migration
  - [`storage-contract`](single/storage-contract) — shared file, Git, SQLite, and Markdown persistence contracts
  - [`release-metadata`](single/release-metadata) — publication metadata and SQLite schema compatibility checks
  - [`maintainer-workflow`](single/maintainer-workflow) — small green commits, acceptance review, and release/security fallback guidance
  - [`stale-lock`](single/stale-lock) — OS-owned lock recovery across Unix, macOS, and Windows
  - [`remove-force-safety`](single/remove-force-safety) — permanent removal, reverse-reference guards, and issued-ID history
  - [`parent-child`](single/parent-child) — parent/child hierarchy
  - [`child-points`](single/child-points) — opt-in recursive parent point aggregation
  - [`labels`](single/labels) — labels and label filters
  - [`sprint-lifecycle`](single/sprint-lifecycle) — sprint close rollover and spillover reporting
  - [`sprint-edit-delete`](single/sprint-edit-delete) — edit a goal-less Sprint, start it, and delete a Sprint while retaining PBIs
  - [`sprint-closed-assignment`](single/sprint-closed-assignment) — reject new assignments to closed Sprints while preserving the rm correction path
  - [`sprint-assignment-validation`](single/sprint-assignment-validation) — validate Sprint IDs and existence consistently across add/edit/sprint add
  - [`sprint-bulk-assignment`](single/sprint-bulk-assignment) — assign ranked PBIs by status with an optional count limit
  - [`sprint-capacity`](single/sprint-capacity) — sprint capacity calculation
  - [`sprint-load-warning`](single/sprint-load-warning) — non-blocking Sprint capacity and velocity warnings
  - [`cycletime`](single/cycletime) — cycle time and lead time
  - [`burndown`](single/burndown) — sprint burndown
  - [`velocity`](single/velocity) — sprint velocity
  - [`next`](single/next) — actionable backlog candidates and dependency readiness
- [`combined/`](combined) — **combined-feature** datasets that resemble real workflows.
  - [`full-scrum-flow`](combined/full-scrum-flow) — one sprint covering epics, dependencies,
    labels, WIP, and every status
  - [`release-train`](combined/release-train) — velocity, burndown, and cycle time across sprints
