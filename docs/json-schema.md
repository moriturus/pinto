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
pinto export --json
```

Do not parse the human-oriented table or board output when `--json` is
available.

## Complete board export

`pinto export --json` returns one read-only object with four fields:

- `items` — the active PBIs, using the same objects and hierarchical priority order as `list --json`.
- `sprints` — all Sprints, using the same objects and creation order as `sprint list --json`.
- `config` — the effective validated board configuration, including defaults for omitted settings.
- `dod` — the shared Definition of Done as Markdown, or `null` when it is unset.

The export acquires the board write lock before opening configuration or the
selected backend and holds it while assembling the complete snapshot. It does
not modify board data or require a server. Ordinary read commands remain
non-blocking and do not provide board-wide snapshot isolation; use
`export --json` when automation needs all resource collections to describe one
board state. Archived PBIs are excluded, matching the default active-backlog
behavior of `list --json`.

## Restoring a board (`import`)

`pinto import <SOURCE>` is the inverse of `export --json`. It reads an export
document (from a file path, or from standard input when `SOURCE` is `-`) and
rebuilds the board's PBIs, Sprints, configuration, and shared DoD. The board
must already be initialized (`pinto init`).

- **Fail-fast on a populated board.** Importing into a board that already holds
  active PBIs or Sprints is refused unless `--force` is given. With `--force`
  the snapshot replaces the existing data: active PBIs and Sprints absent from
  the snapshot are removed, and `config.toml` and the shared DoD are overwritten
  to match. The whole operation runs under the board write lock.
- **Round-trip contract.** `export` → `import` → `export` reproduces the same
  JSON document. Equivalence is defined against this contract, not byte-identical
  storage files.
- **Persistence impact.** Import reuses the existing plain-text persistence.
  Items and Sprints are written to the backend selected by the snapshot's
  `config`, and their IDs are recorded in `issued_ids` so a later `add` never
  reuses a restored ID. No new on-disk format or schema is introduced.
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

`automate --json` returns an object with `status`, `dry_run`, and a `commands`
array. Each command entry includes its one-based `index`, command name, status
(`valid`, `succeeded`, `failed`, or `skipped`), `created_ids`, `updated_ids`,
and an optional localized `error` diagnostic. Error text may also contain the
original stderr from a child command or an external tool, so consumers must use
`status` and the other structured fields rather than parse the error text. A
failed execution stops the plan; later commands are reported as `skipped` so the
applied prefix and the safe recovery point are explicit.

## Automation plan schema

`pinto automate --schema` prints the Draft 2020-12 JSON Schema for the plan
envelope. It is available without an initialized board and does not execute a
plan. The schema requires one or more argv-style command arrays, rejects unknown
top-level properties, and excludes recursive or interactive command names. The
normal CLI parser remains authoritative for the arguments after each command
name, so agents should validate the generated plan with `pinto automate --dry-run`
before applying it.

```bash
pinto automate --schema > automation-plan.schema.json
```
