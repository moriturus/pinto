# Configuration

`pinto init` writes `.pinto/config.toml` with the defaults below. Every setting
is optional to change; a fresh board works without editing anything. Edit the
file directly and keep the change small and reviewable — it is the one file in
`.pinto/` that is meant to be hand-edited.

The CLI discovers this board from descendant directories by walking upward to
the nearest `.pinto/config.toml`. It checks a repository root marked by `.git`
and then stops there, or stops at the filesystem root. Use `pinto --dir PATH`
or `PINTO_DIR=PATH pinto ...` to select a different project (the path may be
the project directory or `.pinto` itself).

```toml
columns = ["todo", "in-progress", "review", "done"]
done_column = "done"

[project]
name = "pinto"
key = "T"

[tui]
confirm_quit = true

[storage]
backend = "file"

[wip]
enabled = true

[display]
markdown = true
timezone = "local"

[points]
aggregate_children = false
```

## Workflow columns

`columns` is the ordered list of Kanban states, left to right. `done_column`
names the completion column; the board sorts that column by completion time and
records a `done_at` timestamp when a PBI enters it. `done_column` must be one of
`columns`, and an unknown value is rejected when the config loads.

Configuration uses a strict schema: unknown keys are rejected with the TOML
table and field path, so a typo such as `[display].timezome` does not silently
fall back to a default. `columns` must contain at least one non-blank, unique
name. Values in `done_column`, `[tui].hidden_columns`, and `[wip.limits]` must
refer to configured columns.

Renaming or removing a column that still holds PBIs strands those items in a
status the workflow no longer recognizes, so move work out of a column before
retiring it.

## Project identity

The `[project]` table sets the display `name` and the PBI ID prefix `key`. With
`key = "T"`, new items are numbered `T-1`, `T-2`, and so on. Changing `key`
affects only IDs assigned afterward; existing IDs keep their original prefix.
The key must contain only ASCII letters. Digits and `-` are reserved for the
numeric ID portion and separator; `_` is not accepted.
The project name must not be empty or whitespace-only. Invalid settings stop
the command before the selected storage backend is opened; fix the reported
field in `config.toml` and retry.

## Storage backend

`[storage] backend` selects where the board is persisted:

- `file` (default) — one Markdown file per PBI under `.pinto/`.
- `git` — the file layout plus one automatic commit for each complete write operation; pre-existing
  Git changes are kept out of that commit.
- `sqlite` — a single `.pinto/board.sqlite3` database, available only in builds
  with the optional `sqlite` feature.

All backends expose the same CLI. Use `pinto migrate --to <backend>` to move an
existing board between them.

Write commands wait up to five seconds for another pinto process by default.
The lock remains held through a Git-backed commit so one service operation stays
atomic. For a slow filesystem or Git hook, set the process environment variable
`PINTO_LOCK_TIMEOUT_SECS` to a larger non-negative integer before running the
command.

## WIP limits

`[wip]` enforces work-in-progress limits per column. It is enabled by default
with no limits set, so nothing is restricted until you add one:

```toml
[wip]
enabled = true

[wip.limits]
in-progress = 3
review = 2
```

Exceeding a limit on `pinto move` prints a warning. Pass `--no-wip-check` to
skip the check for a single move, or set `enabled = false` to disable the check
for the whole board.

## Display

`[display]` controls how PBI bodies and timestamps are shown by `pinto show` and
the Kanban details popup:

- `markdown = true` renders bodies as styled Markdown; set `false` for raw text.
- `timezone` formats human-readable timestamps. Use `local`, `UTC`, or a fixed
  `±HH:MM` offset such as `+09:00`. This affects display only — stored and JSON
  timestamps stay in UTC.

## Parent PBI points (opt-in)

`[points].aggregate_children` is `false` by default. Set it to `true` when parent
PBIs should display the sum of their active descendant leaves:

```toml
[points]
aggregate_children = true
```

When enabled, a parent's stored points are replaced in read-only views while it
has children. A nested parent is counted through its descendants only once, and
an item in `done_column` contributes no points. Active descendants below a
completed intermediate item remain eligible. If an active descendant leaf has
no points, the affected parent is shown as unestimated (`-`) rather than using
an incomplete sum. The stored Markdown frontmatter is never rewritten by this
calculation.

## Interactive Kanban

The `[tui]` table configures the shared parts of the interactive board — exit
confirmation and hidden columns. Personal keybindings are kept outside the
board; see [Kanban (TUI)](kanban.md) for the user configuration and key syntax.
