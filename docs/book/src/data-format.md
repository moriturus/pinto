# Data format

## Board layout

The default file backend stores the board below `.pinto/` in the repository
where pinto is run. The directory contains the configuration, individual PBI
Markdown files, Sprint data, templates, and the `issued_ids` history. Each PBI is a separate file so
that a Git diff shows the change to one item clearly.

The board is local-first: no account, server, or database service is required.
File and Git backends are the plain-text compatibility boundary: their PBI and
Sprint records remain human-readable and a Git diff can show each operation.
SQLite is the explicit persistence exception. It exposes the same pinto
operations through an opt-in, normalized database, but its versioned schema and
migration rules replace the per-record text diff.

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

## Sprint files

A Sprint uses the same TOML-frontmatter/Markdown-body shape under `.pinto/sprints/`. Its title,
state, planned dates, capacity settings, and timestamps are structured fields; its goal is the
Markdown body. Closing a Sprint writes `closed_at` plus `spillover_points`, `spillover_items`, and
`unestimated_spillover_items`. Zero spillover values and an unset close time are omitted before
close. These fields preserve retrospective context after unfinished PBIs are rolled over or
released, while velocity continues to count completed work only.

## Configuration

`.pinto/config.toml` controls the shared workflow and presentation settings. The
default workflow is:

```toml
columns = ["todo", "in-progress", "review", "done"]
done_column = "done"
```

It is the one file under `.pinto/` intended for hand-editing. Beyond the
workflow columns, it selects the storage backend, project identity, WIP limits,
and display/timezone options. Personal interactive Kanban keybindings belong
in `$XDG_CONFIG_HOME/pinto/config.toml`; they are not board data and are not
included in board exports. See [Configuration](configuration.md) for every
setting. Keep machine-readable JSON timestamps in UTC; the display timezone
does not rewrite stored data.

## Compatibility boundaries

Board configuration is a strict TOML schema and may gain keys between releases;
an older binary can reject a newer `.pinto/config.toml`. Markdown PBI and Sprint
records are the file-backed board data. File and Git backends are the plain-text
compatibility boundary, while SQLite is the explicit persistence exception with
its own versioned schema and migration rules. JSON is a machine-readable CLI
output contract, not another persistence backend and not a configuration file.
Personal keybindings are independent of all four board data formats.

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
