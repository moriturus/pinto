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

## Board and PBI commands

Use pinto doctor to check board integrity after hand edits, interrupted migrations, or copied
records. Add --fix to apply only safe mechanical repairs. The command reports references,
relationship cycles, duplicate IDs, issued-ID history, workflow states, rank anomalies, and
tasks/archive filename collisions with a location and repair direction.

| Command | Purpose |
| --- | --- |
| `pinto init` | Initialize a board in the current directory. |
| `pinto add <title>` | Add a PBI; use `--label <label>...` to set one or more labels, or optionally set points, Sprint, body, or a template. |
| `pinto list` | List active PBIs, with status, label, Sprint, search, root-only, long, and JSON filters. Use `--archived` to select archived PBIs. |
| `pinto next` | Show ranked unstarted PBIs whose dependencies are complete. |
| `pinto show <id>...` | Display one or more active PBI details. Use `--archived` to display archived details. |
| `pinto restore <id>` | Restore an archived PBI to the active task store without changing its ID or content. |
| `pinto move <id>... <status>` | Transition one or more PBIs to a workflow column. |
| `pinto reorder <id>` | Reorder a PBI within its sibling group (same parent and column). |
| `pinto edit <id>` | Update PBI fields; `--label <label>...` replaces its labels. With no field, open the configured editor. |
| `pinto remove <id>...` | Archive PBIs; use the `rm` alias and `--force` only for permanent removal. |
| `pinto board` | Render PBIs grouped by workflow column, optionally showing root PBIs only. |
| `pinto kanban` | Open the interactive [Kanban board](kanban.md). |

Examples:

```bash
pinto add "Implement the parser" --label backend cli
pinto list --status todo in-progress --long
pinto list --status todo --long --acceptance-criteria
pinto list --label backend frontend --all-labels
pinto list --search "parser"
pinto list --archived --json
pinto list --roots-only --status todo --json
pinto next
pinto next --count 3 --sprint S-1 --json
pinto board --status in-progress review
pinto board --roots-only --status todo --long
pinto reorder T-1 --top
pinto edit T-1 --title "Implement the Markdown parser" --label backend cli
pinto show T-1 --archived
pinto restore T-1
```

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
pinto sprint start S-1
pinto sprint add S-1 T-1
pinto sprint add S-1 --status todo --limit 3
pinto sprint add S-1 --status todo             # omit --limit to assign all matches
pinto sprint list
pinto sprint close S-1 --rollover S-2          # move unfinished PBIs to S-2
# pinto sprint close S-1 --release             # alternative: clear their Sprint assignment
pinto sprint remove S-1
```

Reports include `pinto sprint burndown`, `pinto sprint velocity`,
`pinto sprint capacity`, and `pinto cycletime`.

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
```

## Automation and shell integration

`automate` accepts a validated JSON plan. Preview a plan before applying any
writes, and use JSON output when another tool needs execution results:

```bash
pinto automate --plan plan.json --dry-run --json
pinto automate --plan plan.json --json
```

Plans can be supplied inline, from a file, or from standard input. `pinto
shell` starts an interactive command shell, and `pinto completion <shell>`
generates completion scripts for supported shells.

The dry-run snapshot holds the board write lock, so a concurrent writer cannot
be mixed into the preview. It works from both normal repositories and linked
worktrees: only `.pinto` is copied, and a temporary owner-private Git
repository is initialized when the source project has Git metadata. The source
`.git` object store is never copied, and the temporary workspace is cleaned up
after success or failure.

## Machine-readable output

Read commands support `--json`:

```bash
pinto list --json
pinto show T-1 T-2 --json
pinto board --json
pinto next --json
pinto sprint list --json
```

Prefer this format over parsing human-oriented tables. IDs, statuses, ranks,
relations, and timestamps keep the same meaning as the regular output;
timestamps are RFC 3339 values in UTC.
