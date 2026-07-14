# Data format

## Board layout

The default file backend stores the board below `.pinto/` in the repository
where pinto is run. The directory contains the configuration, individual PBI
Markdown files, Sprint data, templates, and the `issued_ids` history. Each PBI is a separate file so
that a Git diff shows the change to one item clearly.

The board is local-first: no account, server, or database service is required.
The optional Git and SQLite backends expose the same pinto operations while
keeping the CLI contract consistent.

## PBI files

A PBI file combines TOML frontmatter with a Markdown body:

```markdown
+++
id = "T-1"
title = "Implement the parser"
status = "todo"
rank = "i"
created = "2026-01-01T00:00:00Z"
updated = "2026-01-01T00:00:00Z"
+++

Acceptance criteria and planning notes belong here.
```

The frontmatter carries structured fields such as the ID, title, status, rank,
labels, relations, timestamps, and optional Sprint information. The body is
user-authored Markdown and is preserved when the display locale changes.

The filename stem is part of the record identity: `tasks/T-1.md` and
`archive/T-1.md` must both contain `id = "T-1"`. File reads validate active and
archived items, as well as Sprint filenames, and stop on filename mismatches or
duplicate logical IDs before a write or migration can overwrite existing data.

Statuses must be columns in the configured workflow. The rank is a fractional
index used to keep ordering changes small. Completion and start timestamps are
recorded when a PBI crosses the configured workflow boundaries.

## Configuration

`.pinto/config.toml` controls the workflow and presentation settings. The
default workflow is:

```toml
columns = ["todo", "in-progress", "review", "done"]
done_column = "done"
```

It is the one file under `.pinto/` intended for hand-editing. Beyond the
workflow columns, it selects the storage backend, project identity, WIP limits,
display and timezone options, and the interactive Kanban key bindings. See
[Configuration](configuration.md) for every setting. Keep machine-readable JSON
timestamps in UTC; the display timezone does not rewrite stored data.

## Safe operations

Use pinto commands to add, transition, rank, edit, archive, and relate PBIs.
The generated `.pinto/issued_ids` file preserves every issued item number so a
permanently deleted ID is never assigned to a different PBI; do not remove it
when changing storage backends.
Do not maintain a second hand-edited backlog or edit task files as part of the
normal workflow. Direct recovery is an exception for damaged data; validate the
board with `pinto list` afterward.

For the full JSON contract and migration rationale, see [JSON
output](https://github.com/moriturus/pinto/blob/main/docs/json-schema.md) and
[storage migration](https://github.com/moriturus/pinto/blob/main/docs/migration.md).
