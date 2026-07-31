# JSON output

Read commands support `--json` for automation. JSON is an output format, not a
second persistence model: the Markdown/TOML board files remain authoritative.

## Envelope

Commands return JSON values whose fields describe the requested resource. IDs,
statuses, ranks, timestamps, labels, relations, and bodies use the same meaning
as the normal CLI output. Timestamps are RFC 3339 UTC strings. Optional values
are `null` or omitted where the command has no applicable value. `show --json`
always returns an array, including when one ID is requested.

## Compatibility

pinto is under active development. Consumers should tolerate unknown object
fields and use documented command options rather than depending on display
formatting. The JSON representation is intended for scripts that need stable,
machine-readable values.

The `config` object in `export --json` contains only the effective shared board
configuration. Personal Kanban keybindings from
`$XDG_CONFIG_HOME/pinto/config.toml` are intentionally excluded; JSON output
cannot be used as a user-settings file or as a replacement for Markdown/SQLite
board data.

## Examples

```bash
pinto list --json
pinto show T-1 T-2 --json
pinto board --json
pinto sprint list --json
pinto sprint retro show S-1 --json
pinto sprint retro list --json
pinto sprint review show S-1 --json
pinto sprint review list --json
pinto sprint goal --json
pinto export --json
```

Do not parse the human-oriented table or board output when `--json` is
available.

Every item object includes `source`, which is `null` for an ordinary PBI or an
object such as `{"kind":"review","sprint_id":"S-1"}` for an action PBI
promoted from a Retro or Review. The source link is included in `list`,
`show`, `board`, and `export` item objects and is restored by `import`.

`pinto sprint retro show --json` returns a one-element array, and
`pinto sprint retro list --json` returns an array of Retro objects. A show
object contains `id`, `sprint_id`, `body`, `created`, `updated`, and generated
`context`; list objects contain the record fields only. `sprint_id` is the
explicit parent Sprint reference and currently matches the child record's
stable `id`. The `context.sprint` object
contains the parent Sprint goal, state, planned `start`/`end`, and `closed_at`.
The sibling `capacity`, `velocity`, `burndown`, `cycle_time`, and `spillover`
fields reuse the existing report shapes and are `null` when the corresponding
context is unavailable. `context.spillover` is populated only after close, so
it is not a synthetic zero for a planned or incomplete Sprint. Timestamps are
RFC 3339 UTC strings. A `show` object additionally contains `actions`, an array
of `{id, title, status}` for active PBIs linked to that Retro.

`pinto sprint review show --json` returns a one-element array, and
`pinto sprint review list --json` returns an array of Review objects. A show
object contains the same record fields and generated `context`; list objects
contain the record fields only. `sprint_id` is the explicit parent Sprint
reference and currently matches the child record's stable `id`. A `show` object
additionally contains `actions`, an array of `{id, title, status}` for active
PBIs linked to that Review. The Review has no state of its own:
`context.sprint.state` is the parent Sprint state, and each action's `status` is
the ordinary PBI workflow status.

## Complete board export

`pinto export --json` returns one read-only object with seven fields:

- `items` — the active PBIs, using the same objects and hierarchical priority order as `list --json`.
- `archived_items` — the archived PBIs, using the same objects and rank order as `list --archived --json`.
- `sprints` — all Sprints, using the same objects and creation order as `sprint list --json`.
- `retros` — all Sprint Retros, using the same record fields and creation order as `sprint retro list --json`.
- `reviews` — all Sprint Reviews, using the same record fields and creation order as `sprint review list --json`.
- `config` — the effective validated board configuration, including defaults for omitted settings.
- `dod` — the shared Definition of Done as Markdown, or `null` when it is unset.

The export acquires the board write lock before opening configuration or the
selected backend and holds it while assembling the complete snapshot. It does
not modify board data or require a server. Ordinary read commands remain
non-blocking and do not provide board-wide snapshot isolation; use
`export --json` when automation needs all resource collections to describe one
board state. Archived PBIs are included in `archived_items` so the snapshot is a
lossless board copy: an active PBI may reference an archived parent, and
excluding the archive would let a healthy board export to a snapshot that
reimports as a dangling reference. `archived_items` is an added key, so a
snapshot from an older pinto (which omits it) still imports, restoring no
archive.

## Restoring a board (`import`)

`pinto import <SOURCE>` is the inverse of `export --json`. It reads an export
document (from a file path, or from standard input when `SOURCE` is `-`) and
rebuilds the board's PBIs, Sprints, Sprint Retros, Sprint Reviews, configuration,
and shared DoD. The board must already be initialized (`pinto init`).

- **Fail-fast on a populated board.** Importing into a board that already holds
  active PBIs, archived PBIs, or Sprints is refused unless `--force` is given.
  With `--force` the snapshot replaces the existing data: active PBIs, archived
  PBIs, Sprints, Retros, and Reviews absent from the snapshot are removed, and
  `config.toml` and the shared DoD are overwritten to match. The whole operation
  runs under the board write lock.
- **Consistency check.** Before any write, import rejects a snapshot that would
  produce a board `doctor` flags: a duplicate Sprint, a Sprint that breaks its
  domain invariants (a blank title, a one-sided or inverted period, an active
  Sprint without a Goal, or a Goal outcome recorded without a Goal), a duplicate
  or orphaned Retro or Review, a duplicate PBI ID across the active and archived
  collections, a `parent`, `depends_on`, `sprint`, or action `source` reference
  that resolves to nothing in the snapshot, a PBI with an empty title or a
  `status` that is not a configured workflow column, a `parent` or `depends_on`
  cycle, or a rank reused within an active PBI's `(status, parent)` scope. The
  checks span the active and archived PBIs together and validate `status` against
  the snapshot's own `config` columns, so they mirror `doctor`'s health boundary:
  a healthy `export` always round-trips, and a snapshot that would import as an
  unhealthy board is refused with a non-zero exit and no durable change.
- **Round-trip contract.** `export` → `import` → `export` reproduces the same
  JSON document. Equivalence is defined against this contract, not byte-identical
  storage files. Because the consistency check rejects the only Sprint states the
  persistence layer would silently normalize (a Goal outcome without a Goal), no
  accepted snapshot loses a value on the round trip.
- **Persistence impact.** Import reuses the existing plain-text persistence.
  Items and Sprints are written to the backend selected by the snapshot's
  `config`, and their IDs are recorded in `issued_ids` so a later `add` never
  reuses a restored ID. Retro and Review records remain in their dedicated
  Markdown directories for File, Git, and SQLite. No new on-disk format or
  schema is introduced.
- **Compatibility impact.** Import consumes the stable `export --json` schema
  documented here. Because added keys are non-destructive, a snapshot from an
  older pinto imports into a newer one; capacity inputs (daily hours, holidays,
  deduction factor) are not part of the export contract, so imported Sprints
  restore with capacity unset. `import` is a manual restore and is intentionally
  excluded from `automate` plans.

## Sprint close fields

Every object from `sprint list --json` includes `closed_at`,
`spillover_points`, `spillover_items`, and `unestimated_spillover_items`.
`closed_at` is an RFC 3339 timestamp or `null` until the Sprint closes. The spillover fields are
zero until close, then preserve the estimated points and item counts that were unfinished at that
moment. They are retrospective context and are not included in velocity points, averages, or
change percentages.

## Sprint Goal outcomes

`pinto sprint list --json` includes `goal_achieved` as `true`, `false`, or `null`. A recorded
boolean always accompanies a non-blank Sprint Goal; the result is otherwise independent of PBI
completion, velocity, burndown, capacity, and cycle-time calculations. `sprint edit --goal-achieved
true|false` sets or updates it; `sprint edit --clear-goal-achieved` clears it back to `null`, and
clearing the Goal also clears the result.

`pinto sprint goal --json` returns the selected Sprints and the aggregate fields below:

```json
{
  "sprints": [
    {"id": "S-1", "title": "Sprint 1", "goal_achieved": true}
  ],
  "evaluated_sprints": 1,
  "achieved_sprints": 1,
  "achievement_rate": 100.0
}
```

Only Sprints with a recorded boolean are evaluated. A blank Goal cannot retain a recorded result.
When none are evaluated,
`achievement_rate` is `null`, never `0.0`. Snapshots created before `goal_achieved` was added
remain importable and restore the field as `null`.

`automate --json` returns an object with `status`, `dry_run`, and a `commands`
array. Each command entry includes its one-based `index`, command name, status
(`valid`, `succeeded`, `failed`, or `skipped`), `created_ids`, `updated_ids`,
`resolved_ids`, and an optional localized `error` diagnostic. `created_ids`
contains producer IDs in creation order; `updated_ids` contains the command's
resolved update targets; `resolved_ids` contains every resolved item-ID
argument in argument order. Error text may also contain the original stderr
from a child command or an external tool, so consumers must use `status` and
the other structured fields rather than parse the error text. A failed
execution or placeholder resolution stops the plan; later commands are
reported as `skipped` so the applied prefix and the safe recovery point are
explicit.

## Automation plan schema

`pinto automate --schema` prints the Draft 2020-12 JSON Schema for the plan
envelope. It is available without an initialized board and does not execute a
plan. The schema requires one or more argv-style command arrays, rejects unknown
top-level properties, and excludes recursive or interactive command names. The
normal CLI parser remains authoritative for the arguments after each command
name. A complete item-ID argument may instead use a zero-based output
placeholder in the form `@command[0].created_ids[0]`. The first index selects
an earlier command in the plan, and the second selects one ID from that
producer's structured output. Only successful `add`/`a` or `split`/`spl`
commands can be referenced; the token is never shell-expanded and is accepted
only in item-ID positions. An out-of-range output is reported at execution
time, after which dependent and later commands are skipped.

To pass a placeholder-looking string literally in an ordinary argument such as
`--body`, prefix the marker with a second `@`: write
`@@command[0].created_ids[0]`. Pinto removes one `@` immediately before
executing the command. The escaped form is literal text, while an unescaped
placeholder-like string outside an item-ID position remains invalid.

For example, this plan creates two items, then uses the actual first ID without
predicting the next `issued_ids` number:

```json
{
  "commands": [
    [
      "add",
      "Parent"
    ],
    [
      "add",
      "Child",
      "--parent",
      "@command[0].created_ids[0]"
    ],
    [
      "edit",
      "@command[1].created_ids[0]",
      "--title",
      "Renamed child"
    ]
  ]
}
```

The placeholder is supported in `add --parent`/`--depends-on`, `split`'s
source, `show`, `move`, `reorder` IDs and references, `edit` ID and parent,
`remove`, `restore`, `dep`, `link`, and `sprint add`/`unassign`. A dry-run
resolves the same references against its isolated temporary board; its IDs are
preview values because the real board is not changed. Apply mode resolves them
again against the real board, so a dry-run never supplies authoritative IDs.

```bash
pinto automate --schema > automation-plan.schema.json
```
