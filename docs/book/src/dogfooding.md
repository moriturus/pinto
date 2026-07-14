# Dogfooding

pinto develops on a pinto board. When validating a change in this repository,
run the current worktree through `cargo run` so the behavior under test is the
behavior being developed.

## Inspect the board

Use commands such as these from the repository root:

```bash
cargo run --quiet -- list --status todo --long --json
cargo run --quiet -- show <ID-from-list>
cargo run --quiet -- board
```

Replace `<ID-from-list>` and `<ID>` with IDs returned by the board commands.

Human-readable output is useful for a quick check; `--json` is useful when the
result must be inspected without depending on table formatting.

These examples correspond to the installed `pinto list`, `pinto show`, and
`pinto board` subcommands. The `cargo run --` prefix is intentional while
developing: it selects the executable built from the current checkout.

## Update an item

Add, transition, rank, edit, and remove items through the CLI:

```bash
cargo run -- add "Document the workflow" --template default
cargo run -- move <ID> in-progress
cargo run -- reorder <ID> --top
cargo run -- show <ID>
```

For a transition, the installed form is `pinto move <id> <status>`; the
dogfooding form above is `cargo run -- move <id> <status>`.

Use the default archive operation when an item was created by mistake. Reserve
`remove --force` for an explicit permanent cleanup, and inspect the result
with `list`, `show`, or `board` after every write.

For multiple planned writes, validate the plan first:

```bash
cargo run -- automate --plan plan.json --dry-run --json
cargo run -- automate --plan plan.json --json
```

Do not edit `.pinto/tasks/*.md` directly during normal backlog work. The
configuration file may be edited when changing board settings, but the CLI is
the source of truth for item operations.

## Development verification

After implementation and dogfooding, run the same quality gate used by CI:

```bash
mise run check
mise run release-check
```

The release gate repeats the check as needed and also runs the all-features coverage threshold,
dependency audit, and dependency-policy checks. Coverage has no source exclusions, so storage
boundaries and TUI lifecycle paths remain measured.
