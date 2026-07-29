# CLI reference

Run `pinto --help` or `pinto <command> --help` for the complete, versioned
option list. The commands below cover the normal Scrum workflow.

## Selecting a board

Board commands search the current directory first and then its ancestors for
`.pinto/config.toml`, so they can run from a repository subdirectory. The
search stops after checking a directory that contains `.git` (the documented
repository boundary) or at the filesystem root. From the board root, behavior
is unchanged.

Use `--dir PATH` for scripts and agents when the board is not the nearest one;
`PATH` may name either the project directory or its `.pinto` directory.
`PINTO_DIR` provides the same override when the flag is omitted:

```bash
pinto --dir /work/project list --json
PINTO_DIR=/work/project pinto list --json
```

If no board is found, pinto reports the search and these override options. The
`init` command still initializes the current directory unless an explicit
`--dir` or `PINTO_DIR` target is supplied.

## External commands

Pinto's built-in commands are provided by the single `pinto` binary. An unknown
command is delegated Git-style to an executable named `pinto-<segment>` using
the external command contract described in
[`docs/plugin-contract.md`](../../plugin-contract.md): the directory beside the
running binary wins over `PATH`, empty `PATH` entries never mean the current
directory, arguments are forwarded as argv, and `PINTO_DIR` plus the host and
contract versions are provided to the child process. A built-in command name is
always resolved by pinto itself and is never shadowed by a same-name executable
on `PATH`.

## Board and PBI commands

Use `pinto doctor` to check board integrity after hand edits, interrupted migrations, or copied
records. Add `--fix` to apply only safe mechanical repairs. The command reports references,
relationship cycles, duplicate IDs, issued-ID history, workflow states, rank anomalies, and
tasks/archive filename collisions with a location and repair direction. Both modes inspect the
board once up front; `--fix` re-inspects only after it applied a repair.

| Command | Purpose |
| --- | --- |
| `pinto init` | Initialize a board in the current directory. |
| `pinto add <title>` | Add a PBI; use `--label <label>...` to set one or more labels, or optionally set points, Sprint, body, or a template. |
| `pinto split <id> <title>...` | Split a PBI into new PBIs; optionally make the source their parent or dependency and choose the body. |
| `pinto list` | List active PBIs, with status, assignee, label, Sprint, search, stale-duration, root-only, long, and JSON filters. Use `--archived` to select archived PBIs. |
| `pinto next` | Show ranked unstarted PBIs whose dependencies are complete. |
| `pinto show <id>...` | Display one or more active PBI details. Use `--archived` to display archived details. |
| `pinto restore <id>` | Restore an archived PBI to the active task store without changing its ID or content. |
| `pinto move <id>... <status>` | Transition one or more PBIs to a workflow column. |
| `pinto reorder <id>` | Reorder a PBI within its sibling group (same parent and column). |
| `pinto edit <id>` | Update PBI fields; `--label <label>...` replaces its labels. With no field, open the configured editor. |
| `pinto remove <id>...` | Archive PBIs; use the `rm` alias and `--force` only for permanent removal. |
| `pinto board` | Render PBIs grouped by workflow column, optionally filtering by assignee or showing root PBIs only. |
| `pinto export --json` | Export the complete active board, configuration, and shared DoD as one consistent JSON snapshot; it waits for writers. |
| `pinto doctor` | Check board integrity; use `--fix` for safe mechanical repairs. |
| `pinto kanban` | Open the interactive [Kanban board](kanban.md). |

Examples:

```bash
pinto add "Implement the parser" --label backend cli
pinto list --status todo in-progress --long
pinto list --status todo --long --acceptance-criteria
pinto list --label backend frontend --all-labels
pinto list --assignee alice --json
pinto list --search "parser"
pinto list --stale 7d --status todo --json
pinto list --archived --json
pinto list --roots-only --status todo --json
pinto next
pinto next --count 3 --sprint S-1 --json
pinto board --status in-progress review
pinto board --assignee alice --json
pinto board --roots-only --status todo --long
pinto export --json
pinto reorder T-1 --top
pinto edit T-1 --title "Implement the Markdown parser" --label backend cli
pinto show T-1 --archived
pinto restore T-1
pinto split T-1 "Cart page" "Payment step" --child
pinto split T-1 "Payment spike" --dependency --body "Evaluate providers."
```

### Split a PBI

`pinto split <source> <title>...` derives one new PBI per title from an existing
PBI. The source item is kept; each new PBI is appended to the backlog in the
first workflow column.

Choose at most one relationship between the source and the new PBIs:

- `--child` makes the source the parent of each new PBI.
- `--dependency` makes the source depend on each new PBI (the new work must be
  completed first).

Choose at most one body; the default copies the source body:

- `--body <text>` uses the supplied text.
- `--template <name>` uses `.pinto/templates/item/<name>.md`.
- `--empty` starts each new PBI with an empty body.

The same operation is available inside the [Kanban board](kanban.md) with the
`s` key.

### Multi-record recovery

`split` and `import --force` are single operation-level mutations. Pinto
prepares the complete record set before writing it and keeps a pre-operation
recovery point. If a record or metadata write fails, File, Git, and SQLite
restore the board to the state that existed before the command and report that
the operation can be retried.

SQLite applies the PBI, relationship, and Sprint portion of each operation in
one database transaction. The shared configuration, DoD, and issued-ID
history are covered by the surrounding recovery protocol because they are
stored outside the database.

The Git backend has one additional boundary: if the final Git commit fails
after the board files were written, Pinto leaves the complete change in the
worktree so it is recoverable. Run `git status`, fix the reported hook or Git
problem, then retry the command or commit the durable `.pinto` changes
manually. Do not discard the worktree before inspecting it.

If automatic restoration itself fails, the error retains the pre-operation
snapshot in a temporary directory and prints its path. Stop other writers,
preserve `.pinto/.lock`, restore the retained snapshot into `.pinto/`, inspect
the board, and retry only after the board is coherent again.

```bash
# A record-write failure reports that the board was restored; retry the command.
cargo run --manifest-path ../../../Cargo.toml -- split T-1 "Retry the slice"

# After a Git commit failure, inspect and repair the durable board change.
git status --short
cargo run --manifest-path ../../../Cargo.toml -- import --force snapshot.json
```

### Consistent board reads

`list`, `show`, `board`, `next`, and the other ordinary read commands do not
take the board-wide write lock. This keeps them non-blocking, but they do not
provide snapshot isolation when a write operation is running; separate
resources read by one command may come from different versions of the board.

For shell scripts, agents, and other automation that must correlate PBIs,
Sprints, configuration, and the shared Definition of Done, use
`pinto export --json`. Export waits for a writer, acquires the board lock
before opening configuration and storage, and holds it while assembling one
complete snapshot.

For `add` and `edit`, multiple label values may follow one `--label`; repeating
the option once per value remains equivalent. The `list` and `board` forms are
label filters and keep their documented OR/AND behavior.

### Display order

Priority is **hierarchical**. Every view — `list`, `board`, `kanban`, and their
`--json` output — flattens the same parent/child forest in one canonical order:

1. Top-level PBIs come first, in ascending `rank` (with a `(prefix, number)` ID
   tie-break so equal ranks never reorder between views).
2. Each parent is immediately followed by its whole subtree; a parent's children
   are ordered among themselves by `rank`.

So **`rank` orders siblings, and the tree decides the overall priority**: a
child never floats above an unrelated, higher-priority PBI just because its raw
`rank` string happens to be lower. Deprioritise a parent and its entire subtree
moves with it.

- `pinto list` flattens the whole forest. A filtered-out or absent parent
  promotes its children to the top level, so the tree is cut cleanly at the
  filter boundary.
- `pinto board` and `pinto kanban` build the same forest **per column**. A child
  whose parent lives in another column is shown at the top level of its own
  column (positioned by its own `rank`).
- The completion column (`done_column` in `config.toml`) orders its top-level
  and sibling groups by completion time (`done_at`) descending by default, so
  the most recently finished PBI leads; the subtree grouping still applies.
- `pinto board --sort rank | done | created` sets the root/sibling order
  explicitly (add `--reverse` to invert it); the hierarchy is always preserved.
  `pinto kanban` uses the defaults and has no sort toggle.

Because `rank` is sibling-local, `pinto show` and the Kanban details popup print
it as a sibling ordinal: `#2 under <parent-id>` for a child (2nd among that
parent's children)
or `#2` for a top-level PBI.

### Root-only views

Use `--roots-only` with `list` or `board` to show only PBIs whose persisted
`parent` field is unset. Child PBIs are omitted, while root PBIs with or without
children remain visible. Without the option, the existing hierarchical output
is unchanged.

The option composes with compatible filters and output modes, for example:

```bash
pinto list --roots-only --status todo --label backend --search parser --json
pinto board --roots-only --status todo --sort rank --reverse --long
```

The check uses the stored parent link, not just the current result set. Thus a
child is still omitted when its parent is hidden by a status, Sprint, label, or
search filter.

The [`parent-child` demo](https://github.com/moriturus/pinto/tree/main/demos/single/parent-child)
contains a reproducible hierarchy for trying these commands.

### Assignee filters

Use `--assignee <name>` (or `-u <name>`) with `list` or `board` to keep only PBIs whose persisted
assignee exactly matches the requested name. The filter composes with status, Sprint, label, and
search filters, and applies before hierarchical ordering or board-column grouping. It also works
with `--json`; omitting it leaves the existing result set and order unchanged.

The [`status-filter` demo](https://github.com/moriturus/pinto/tree/main/demos/single/status-filter)
includes assigned PBIs across multiple workflow columns.

### Stale PBIs

`pinto list --stale <duration>` matches PBIs whose `updated` timestamp is at or before the query
time minus the supplied duration. Use a positive integer with a single unit: `s` for seconds, `m`
for minutes, `h` for hours, `d` for days, or `w` for weeks. For example, `7d` finds PBIs unchanged
for at least seven days. The filter composes with the other list filters and with long or JSON
output, and it performs no writes.

The [stale-filter demo](https://github.com/moriturus/pinto/tree/main/demos/single/stale-filter)
contains a small board for trying the command.

### Archived PBIs

`pinto rm` archives a PBI in `.pinto/archive/` by default. Archived records are
excluded from normal `list`, `board`, and `show` views. Select them explicitly
when reviewing recovery candidates:

```bash
pinto list --archived
pinto show T-1 --archived
pinto restore T-1
```

Restore preserves the archived Markdown, ID, rank, and relationships. It checks
the active task store first and refuses an ID collision without overwriting
either record.

### Actionable candidates

Use `pinto next` to find work that can start immediately. An item is unstarted when it is in the
first configured workflow column, and it is actionable when every declared dependency exists and
is in `done_column`. Items already in progress, in review, or in the completion column are not
returned; a missing or unfinished dependency keeps an item blocked.

The command is read-only and follows the canonical backlog order. `--count` (or `-n`) limits the
number of candidates and defaults to `1`; `--sprint` (or `-S`) restricts the exact Sprint ID;
`--json` emits the same PBI object array used by `list --json`:

```bash
pinto next
pinto next --count 3
pinto next --sprint S-1 --json
```

The [`next` demo](https://github.com/moriturus/pinto/tree/main/demos/single/next) contains blocked,
ready, completed, and already-started examples.

### Acceptance Criteria progress

Pinto derives a `completed/total` value from Markdown task-list checkboxes in the PBI body. The
value appears in `pinto show` and the Kanban details popup. Add `--acceptance-criteria` (or `-A`)
to `list --long` or `board --long` to include it as a column. No progress field is persisted and
the body is not rewritten.

When a move enters the configured `done_column`, an item with unchecked task-list boxes produces a
warning on stderr but the transition remains successful. An item with no task-list boxes does not
produce this warning. See the [Acceptance Criteria demo](https://github.com/moriturus/pinto/tree/main/demos/single/acceptance-criteria)
for a runnable example.

A move keeps the item's rank, so its relative position travels with it into the new column. The one
exception is a rank that already exists in the destination column: to keep ranks unique within a
column, the item is re-pegged to the column's tail instead.

`pinto reorder` (and Kanban `K` / `J`) moves a PBI only **within its sibling
group** — `--top` / `--bottom` go to the front/back of that group, and
`--before` / `--after` take a sibling as reference. Reordering relative to a
non-sibling is refused; move a PBI between groups with `edit --parent`. Moving a
parent carries its whole subtree.

## Relations and Sprints

Use dependency commands to record ordering constraints between PBIs:

```bash
pinto dep add T-2 T-1
pinto dep rm T-2 T-1
```

Git commit links are managed separately:

```bash
pinto link add T-1 abc1234
pinto link sync
```

The Sprint commands create and manage time-boxed work:

```bash
pinto sprint new S-1 "Sprint 1" --goal "Ship the parser" --start 2026-07-01 --end 2026-07-14
pinto sprint edit S-1 --goal "Ship the parser" --start 2026-07-01 --end 2026-07-14
pinto sprint edit S-1 --goal-achieved true     # record the retrospective outcome
pinto sprint edit S-1 --goal-achieved false    # update it when the assessment changes
pinto sprint edit S-1 --clear-goal-achieved    # return to unevaluated
pinto sprint start S-1
pinto sprint add S-1 T-1
pinto sprint add S-1 --status todo --limit 3
pinto sprint add S-1 --status todo             # omit --limit to assign all matches
pinto sprint list
pinto sprint retro new S-1 --body "What went well\nWhat to improve"
pinto sprint retro show S-1 --json
pinto sprint retro edit S-1 --body "Updated retrospective notes"
pinto sprint retro list --json
pinto sprint close S-1 --rollover S-2          # move unfinished PBIs to S-2
# pinto sprint close S-1 --release             # alternative: clear their Sprint assignment
pinto sprint remove S-1
```

Reports include `pinto sprint burndown`, `pinto sprint velocity`,
`pinto sprint capacity`, `pinto sprint goal`, and `pinto cycletime`.

`pinto sprint goal` reports the explicit boolean outcome for the most recent five Sprints and
calculates `achieved evaluated / all evaluated` as a percentage. A Sprint with a blank Goal or no
recorded outcome is shown as unevaluated and is excluded from the denominator. Use `--recent N`
to select a different number of Sprints and `--json` for the machine-readable fields
`goal_achieved`, `evaluated_sprints`, `achieved_sprints`, and `achievement_rate`. The rate is
`null` (human output: `n/a`) when no Sprint Goal has been evaluated.

After a successful `pinto sprint start` or `pinto sprint add`, pinto prints a non-blocking warning
to stderr when the Sprint's estimated assigned points exceed either its configured capacity-hours
value or the average completed points from its five most recent closed predecessor Sprints.
Unestimated PBIs do not contribute to the point total, equality is within the threshold, and no
warning is emitted when the corresponding comparison is unavailable.

Use `pinto sprint edit` to add a goal or change a planned period before
starting a Sprint. Removing a Sprint releases its assigned PBIs without
deleting them. Assign new PBIs only to `planned` or `active` Sprints; use
`pinto sprint unassign` to correct an assignment that remains after a Sprint closes. Close changes
only unfinished PBIs. `--rollover` and `--release` are mutually exclusive, while omitting both
retains assignments. Completed PBIs remain untouched.

`pinto sprint retro` manages at most one Markdown Retro per Sprint. The record
is stored as `.pinto/retro/<SPRINT-ID>.md`, independent of the Sprint state, so
it can be created for a planned, active, or closed Sprint. Use
`pinto sprint retro new <SPRINT-ID> --template <NAME>` to load
`.pinto/templates/retro/<NAME>.md`; adding `--edit` opens the standard editor
with that template as the initial body. The direct creation form
`pinto sprint retro <SPRINT-ID>` is also accepted.

Velocity totals, averages, and changes count only PBIs completed by the actual close time.
Close-time unfinished points and item counts are displayed separately as spillover and never added
to velocity, even if retained work reaches Done later.

## Definition of Done

A single Definition of Done is shared by every PBI. Display, set, or clear it:

```bash
pinto dod                          # show the current shared DoD
pinto dod set "- [ ] Tests pass and docs updated"
pinto dod clear
```

The DoD body is stored verbatim, so pass a multi-line checklist with a real
newline in the quoted string. Because the text often starts with a hyphen, it is
taken as a literal value rather than an option.

## Maintenance

These commands keep storage tidy and are not part of the daily loop:

```bash
pinto rebalance --dry-run          # preview oversized sibling scopes and shorter ranks
pinto rebalance                    # rewrite only scopes that need it
pinto migrate --to git             # switch the storage backend
pinto import snapshot.json         # restore a board from an export --json snapshot
pinto import --force snapshot.json # replace an existing non-empty board
pinto undo                         # revert the most recent completed mutation (git backend)
```

`pinto import` is the inverse of `pinto export --json`: it rebuilds the PBIs,
Sprints, configuration, and shared DoD from a snapshot (a file, or `-` for
standard input). Importing into a board that already holds PBIs or Sprints is
refused unless `--force` is given. See
[JSON output](https://github.com/moriturus/pinto/blob/main/docs/json-schema.md)
for the round-trip contract.

### Undoing the last mutation

`pinto undo` reverts the most recent completed board mutation. It is a guided,
one-level recovery for a mistaken `move`, `edit`, or `rm --force`, and it only
works on the **git backend**, where each mutation is recorded as a
`pinto: <verb> <id>` commit:

```bash
pinto undo   # git revert HEAD, recorded as a new "Revert ..." commit
```

Undo creates a *new* commit that reverses the last one (it never rewrites
history), so the undo itself is reviewable with `git diff` and can be undone in
turn. It refuses when the latest commit was not made by pinto — for example a
user commit stacked on top of the board — and points at `git log -- .pinto` so
you can revert the right commit by hand.

On the historyless backends (`file`, `sqlite`) there is nothing to revert, so
`pinto undo` fails with exit code 1 and explains the recovery options: restore
from a backup or version-control checkout, or switch to
`[storage] backend = "git"` to enable undo for future mutations. The rationale
and per-backend contract live in
[Undoing a mutation](undo.md).

## Automation and shell integration

`automate` accepts a validated JSON plan. Preview a plan before applying any
writes, and use JSON output when another tool needs execution results:

```bash
pinto automate --schema
pinto automate --plan plan.json --dry-run --json
pinto automate --plan plan.json --json
```

`--schema` prints the machine-readable JSON Schema without requiring an
initialized board or an execution plan. It describes the required non-empty
`commands` array, rejects unknown top-level fields and recursive or interactive
commands, and leaves each command's full argument grammar to the normal CLI
parser. Plans can be supplied inline, from a file, or from standard input.
`pinto shell` starts an interactive command shell, and `pinto completion <shell>`
generates completion scripts for supported shells.

An earlier successful `add` or `split` command can expose its created IDs to
later commands with a complete item-ID placeholder:

```json
{
  "commands": [
    ["add", "Parent"],
    ["split", "@command[0].created_ids[0]", "Slice A", "Slice B"],
    ["edit", "@command[1].created_ids[0]", "--title", "Renamed slice"]
  ]
}
```

Both indexes are zero-based. The command index refers to the earlier plan
command, and the output index refers to its `created_ids` array. Placeholders
are substituted as argv values, never passed through a shell, and are accepted
only in item-ID positions: add parent/dependencies, split source, show, move,
reorder, edit ID/parent, remove, restore, dep, link, and sprint add/unassign.
Unknown, future, malformed, or out-of-range references fail the dependent
command and skip the remaining plan. Dry-run resolves references in the
isolated preview board; IDs in a dry-run report are preview values.

To pass a placeholder-looking string literally in an ordinary argument such as
`--body`, prefix the marker with a second `@`: write
`@@command[0].created_ids[0]`. Pinto removes one `@` immediately before
executing the command. The escaped form is literal text, while an unescaped
placeholder-like string outside an item-ID position remains invalid.

The dry-run snapshot holds the board write lock, so a concurrent writer cannot
be mixed into the preview. Use `pinto export --json` for the same consistency
boundary when an automation consumer needs a complete active-board read. It
works from both normal repositories and linked
worktrees: only `.pinto` is copied, and a temporary owner-private Git
repository is initialized when the source project has Git metadata. The source
`.git` object store is never copied, and the temporary workspace is cleaned up
after success or failure.

`--json` reports producer IDs in `created_ids`, resolved update targets in
`updated_ids`, and every resolved item-ID argument in `resolved_ids`. Apply
results contain authoritative IDs from the real board; dry-run results are
explicitly marked with `dry_run: true` and must not be used as apply IDs.

## Machine-readable output

Read commands support `--json`:

```bash
pinto list --json
pinto show T-1 T-2 --json
pinto board --json
pinto next --json
pinto sprint list --json
pinto export --json
```

Prefer this format over parsing human-oriented tables. IDs, statuses, ranks,
relations, and timestamps keep the same meaning as the regular output;
timestamps are RFC 3339 values in UTC.
