# Quick start

The following workflow creates a board, adds a PBI, inspects it, and moves it
through the default workflow. Run it from the directory that should own the
board.

## 1. Initialize a board

```bash
pinto init
```

Initialization is idempotent. It creates the local `.pinto/` board and its
default workflow when they do not exist.

## 2. Add a PBI

```bash
pinto add "Implement the Markdown parser" --points 3 --label backend
```

The command prints the newly assigned ID, for example `T-1`. Keep that ID for
the following commands.

## 3. Inspect the backlog

```bash
pinto list
pinto show T-1
```

Use `--long` for status, points, assignee, and timestamps. Add
`--acceptance-criteria` when you want the Markdown checklist progress, or use
`--json` when a script needs machine-readable output:

```bash
pinto list --status todo --long
pinto list --status todo --long --acceptance-criteria
pinto show T-1 --json
```

## 4. Move the work

The last operand to `move` is the destination column:

```bash
pinto move T-1 in-progress
pinto move T-1 review
pinto move T-1 done
```

The configured workflow determines which columns are valid. The default
columns are `todo`, `in-progress`, `review`, and `done`.

## 5. View the board

```bash
pinto board
```

For an interactive view, use `pinto kanban`. The Kanban view uses the same
board and services as the non-interactive commands, so changes remain visible
to both interfaces.
