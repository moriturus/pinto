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

## Examples

```bash
pinto list --json
pinto show T-1 T-2 --json
pinto board --json
pinto sprint list --json
```

Do not parse the human-oriented table or board output when `--json` is
available.

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
