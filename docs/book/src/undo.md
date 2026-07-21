# Undoing a mutation

`pinto undo` reverts the most recent completed board mutation. This page is the
feature decision record for that command: its scope, its per-backend behavior,
and its compatibility impact.

## Scope

Undo is a **guided, one-level** recovery. It targets the single most recent
completed mutation — the kind of mistake that `rm --force`, a wrong `move`, or an
unintended `edit` produces — and nothing deeper. Walking further back through the
history stays a manual Git task, which keeps the command lightweight and its
behavior predictable.

Undo is deliberately excluded from `pinto automate` plans. Reverting a mutation
is a human corrective action; an agent plan should not reverse its own earlier
commands.

## Per-backend behavior

pinto only records history on the git backend, so recovery is backend-specific.

### Git backend

Each board mutation is one `pinto: <verb> <id>` commit, so the most recent
mutation is the current `HEAD`. `pinto undo` runs `git revert --no-edit HEAD`,
which writes a **new** commit that reverses the change:

```bash
pinto undo
# Reverted the most recent board mutation: pinto: add T-3
```

Revert — not `reset` — was chosen on purpose:

- It is **non-destructive**: history is preserved, so nothing is lost and the
  operation is safe on a shared board.
- It is **Git-friendly and reviewable**: the undo lands as a
  `Revert "pinto: …"` commit whose effect you can inspect with `git diff`.
- It is **reversible**: undoing the undo is just another revert.

Undo refuses, without touching the repository, when there is nothing pinto can
revert:

- the repository has no commits yet, or
- the latest commit is **not** a pinto board mutation (its subject does not
  start with `pinto: `) — for example a user commit stacked on top of the board,
  or a previous `undo`'s own revert commit.

In the second case the message names the offending commit and points at
`git log -- .pinto` and a manual `git revert <sha>`, so undo never silently
reverses an unrelated commit.

Like every other mutation, undo runs under the board write lock, so it is
serialized against concurrent writers.

### File and SQLite backends

These backends keep no history, so there is nothing to revert. `pinto undo`
fails fast with exit code 1 and an actionable message that names the current
backend and lists the recovery options:

- restore the affected files from a backup or a version-control checkout, or
- switch to the git backend (`pinto migrate --to git`, or set
  `[storage] backend = "git"`) to enable undo for future mutations.

## Compatibility and persistence impact

The command is purely additive:

- **No data-format or schema change.** Undo reuses the existing plain-text
  persistence and the established `pinto: <verb> <id>` commit convention.
- **No migration.** Boards created before this command work unchanged; undo only
  reads existing history and appends a revert commit.
- **No new dependency.** It runs through the same `git` subprocess helpers the
  git backend already uses.

## Try it

The [`undo` demo](https://github.com/moriturus/pinto/tree/main/demos/single/undo)
ships a reproducible git-backed board you can revert and inspect.
