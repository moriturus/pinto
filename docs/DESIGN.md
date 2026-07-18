# pinto design

## Purpose

pinto is a local-first Scrum board for the terminal. Its core concepts are
Product Backlog Items (PBIs), Sprints, and a configurable Kanban workflow.
Board data is human-readable text intended to be reviewed with Git.

## Design principles

1. Keep startup, configuration, and dependencies small.
2. Keep the product focused on Scrum execution.
3. Store durable data as plain text and make changes diff-friendly.
4. Require neither a service nor an account.

The project intentionally excludes general project-suite features, cloud sync,
and any setup process that is required before a user can create a local board.

## Architecture

The binary is organized into layers:

- `cli` parses commands, localizes user-facing text, and renders output.
- `service` coordinates domain operations and persistence.
- `backlog`, `sprint`, and `rank` contain domain types and pure logic.
- `storage` provides file, Git, and optional SQLite repository backends.

Domain logic must not depend on CLI or TUI types. Public, non-obvious APIs use
Rust documentation comments. Errors are `thiserror` values in library layers
and are contextualized with `anyhow` in the binary.

## Data model

Each PBI has an ID, title, status, fractional rank, optional points, labels,
assignee, Sprint, parent, dependencies, timestamps, linked commits, and a
Markdown body. A board stores each PBI separately. Sprints have an ID, title,
state, goal, schedule, and capacity inputs. A goal may be added while
planned, but a Sprint cannot start until its goal is non-blank. The title is
stored in frontmatter and the goal is stored as the Markdown body after
frontmatter. Closing records the actual close time and a snapshot of unfinished estimated points,
item count, and unestimated item count. Rollover and release change only unfinished assignments;
completed PBIs are left untouched. Velocity remains completed work only, with the spillover
snapshot exposed separately for retrospectives.

The default workflow is `todo`, `in-progress`, `review`, and `done`. A board
can configure its columns and the name of its done column. Invalid legacy
statuses remain visible rather than being discarded.

### Rank ordering

PBIs must be reorderable without rewriting every item in a workflow column or
unrelated parent/child group. Integer positions would force broad updates when
an item is inserted between two neighbors, so pinto uses lexicographically
sortable fractional index strings. A new rank is derived between adjacent
ranks, which normally changes only the moved PBI.

Ranks are local to a sibling scope: items share a rank axis only when both
`status` and `parent` match. Ranks use a canonical base-36 representation, and
generated values do not end in `0`; this keeps equivalent values from having
multiple spellings. Rebalance uses the smallest fixed-width, evenly spaced
canonical ranks for a scope only when its maximum rank length exceeds that
width. Unchanged scopes and items are not rewritten.

Normal reordering produces small Git diffs and works well with text storage.
Repeated insertions in the same narrow interval can increase rank length, so
rank metrics and explicit rebalance support are retained. Tail appends use a
short rank path to keep rank growth bounded, while explicit rebalance remains
available for oversized scopes.

## Persistence

The default backend writes Markdown files with TOML frontmatter under `.pinto`.
Writes use atomic replacement and an OS-level advisory lock on `.pinto/.lock`
so concurrent commands do not lose updates. The lock is owned by the open file
handle, not by the PID text stored for diagnostics, so process termination
releases it without PID-reuse guesses. The Git backend writes through the file
backend and finishes each complete service operation with one commit boundary.
It builds that commit through a temporary Git index rooted at `HEAD`, copies
only paths that became dirty after the operation opened the backend, and
restores the caller's pre-existing staged index entries afterwards. This keeps
unrelated staged, unstaged, and untracked changes inside or outside `.pinto`
out of pinto history. The transient `.pinto/.lock` is never committed; a legacy
checkout that tracked it has the path removed from the next pinto commit while
the live lock handle remains held. Direct board-file mutations such as the
common DoD and migration configuration switch use the same boundary. If the
Git commit itself fails, the durable file changes remain in the worktree and
the real index is untouched; inspect/fix Git and retry or commit the files
manually. A failure while refreshing the real index is reported after `HEAD`
has been updated, so `git status` remains the recovery source of truth.
The optional SQLite backend normalizes persisted board entities while
exposing the same repository behavior.

Configuration is TOML. User-created task bodies and existing board data are
never translated or rewritten merely because the application locale changes.
Shared board configuration stays in `.pinto/config.toml`; personal Kanban key
bindings use the same `[tui.key_bindings]` table under
`$XDG_CONFIG_HOME/pinto/config.toml` (with the standard home fallback). The
key parser is shared by user-configuration validation and the TUI event loop:
it supports named keys, modifier combinations including platform
Command/Super, and multiple aliases per operation. Shift remains available for named keys such
as `Shift+Left`; printable results are written directly (`A` or `<`, not `Shift+a` or
`Shift+,`), including after another modifier (`Ctrl+A`, not `Ctrl+Shift+a`). The footer fixes
the five primary operation groups (cursor movement, expand, details, normal quit, and help);
every other accepted operation key is listed in the `?` help window.
The help window is a non-modal overlay, so those operations remain available while it is open and
close it once accepted; pressing `?` closes it without running another operation.
Omitted operations use the built-in defaults, including `?` for help and `Ctrl+?`
for regular-expression search. The `add`,
`dependency_add`, `dependency_remove`, and `parent` actions open in-view prompts
and call the same item/dependency/item-edit services as the CLI. Relation forms
accept either a typed ID or a cursor-selected card followed by `Enter`; the
parent form clears the parent when submitted on the source card with an empty
buffer. `Esc` cancels before any service call, and user-facing validation errors
leave the form and selection intact.

The `[display].timezone` setting affects only human-readable timestamp rendering.
It accepts `local` (the default), `UTC`, or a fixed `±HH:MM` offset. Stored
timestamps and JSON output remain UTC RFC3339 values, so display preferences do
not alter board data or machine-readable contracts. The same setting is used by
long list/board views, Sprint periods, PBI details, and the Kanban details popup.

## Concurrency

Use Tokio for file and process I/O, with the multi-thread runtime. When several
resources are needed, collect them concurrently with `JoinSet`. Use Rayon for
CPU-bound work such as parsing or aggregating many PBIs. The boundary is:
collect asynchronously, then compute in parallel. Do not add a size-based
synchronous fallback.

Write operations acquire the board lock before loading `config.toml` or opening
the selected backend. Migration holds that same lock through destination writes
and the configuration switch, so a writer that waited for migration selects and
writes to the migration target rather than caching the old backend. Ordinary
read commands do not acquire the board-wide lock, which keeps them non-blocking
but means that a multi-resource read has no snapshot isolation while a writer is
active. The complete-board `export --json` workflow is the exception: it acquires
the same lock before loading configuration or opening storage and holds it while
copying PBIs, Sprints, configuration, and the shared DoD into one snapshot. Shell
and agent automation that needs those resources to agree must use `export --json`.

## CLI and localization

`clap` derives the command interface. English is the fallback locale; supported
locales are selected through `LC_ALL` and then `LANG`. Fluent resources keep
runtime text separate from command behavior. Help, errors, board legends, and
TUI labels use the selected locale. Every pinto-owned CLI message and structured
domain-error variant is resolved through the catalog; the error variant's stable
code selects its message rather than translating only the common `error:` prefix.
Diagnostics supplied by the operating system, Git, TOML, or another dependency
are inserted verbatim into the localized wrapper so their actionable source text
is preserved.

The interactive Kanban view uses `ratatui` and `crossterm`. Terminal setup and
input polling run on a blocking thread, while service operations retain their
asynchronous I/O path.

## Testing and quality

Development follows Red → Green → Refactor. Unit tests cover domain behavior;
integration tests cover CLI behavior and localized output. Before committing,
run `mise run check`, which runs tests (with all features), Clippy with warnings
denied, rustdoc and mdBook builds with warnings denied, and format checking. Test assertion
messages may stay in the contributor's language; only shipped runtime text and
public documentation are held to the English requirement below.

## OSS publication checklist

- Public documentation, examples, and contributor guidance use English.
- Public API documentation and non-obvious implementation comments use English.
- English CLI output is tested; localized runtime resources remain intentional.
- User-created board data is not translated or modified.
- Run a repository text scan for unintended Japanese fixed text, excluding
  `.pinto`, tests, and intentional locale resources.
