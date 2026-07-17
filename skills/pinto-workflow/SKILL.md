---
name: pinto-workflow
description: Manage local pinto Scrum backlogs and Kanban boards from the terminal. Use when a user asks to initialize or inspect a `.pinto/` board; add, edit, prioritize, relate, transition, or archive Product Backlog Items; manage Sprints or the shared Definition of Done; query pinto as JSON; or preview and apply a multi-command automation plan.
---

# Pinto Workflow

Use the installed `pinto` binary to manage the board in the user's current project. Keep the workflow lightweight, Scrum-focused, local-first, and reviewable in Git.

## Match intent to authority

- Treat inspection, explanation, summarization, and status requests as read-only. Do not initialize or change the board for those requests.
- Run write commands only when the user requests a board change or the requested outcome unambiguously requires one. A dry-run authorizes only the preview, not the real write.
- Do not transition a PBI merely because it was inspected, discussed, or implemented outside pinto. Move it only when the user's request explicitly or unambiguously includes that workflow change.
- Treat `link sync` as a write command because it associates matching commits with PBIs.

## Enter the correct board

1. Before the first board operation in a session, run `pinto --version` to confirm that the installed binary is available. If it is unavailable, report the blocker and stop; do not build, install, or substitute another runner unless the user requests setup.
2. Confirm the intended project directory before running a write command.
3. Look for `.pinto/config.toml`. If it is absent, run the idempotent `pinto init` only when the user wants a new board. If an existing board was expected, locate it or ask for the correct directory instead of initializing the wrong one.
4. Read `.pinto/config.toml` when exact workflow columns or board settings matter. Do not assume the default status names on a customized board.
5. Run `pinto --help` or `pinto <command> --help` before using an unfamiliar option. Treat the installed CLI help as authoritative for that version.

## Inspect before changing

Prefer structured output for agent decisions:

```bash
pinto list --json
pinto show T-1 --json
pinto board --json
pinto sprint list --json
pinto dod
```

Use IDs returned by pinto instead of guessing the board's ID prefix. `show --json` always returns an array, even for one ID. Do not parse human-oriented tables when `--json` is available.

Narrow large boards with supported filters:

```bash
pinto list --status todo in-progress --json
pinto list --label backend frontend --json
pinto list --label backend frontend --all-labels --json
pinto list --roots-only --status todo --json
pinto list --search "parser" --json
```

Treat multiple labels as OR unless `--all-labels` requests AND. Preserve pinto's returned hierarchical order instead of sorting raw rank strings yourself: rank compares siblings within the same parent and status, and a parent carries its subtree. Expect the configured completion column to use completion time by default rather than backlog rank.

Before acting on a selected PBI, inspect its body, acceptance criteria, relations, Sprint assignment, and the shared Definition of Done:

```bash
pinto show T-1
pinto dod
```

## Change PBIs deliberately

Create PBIs with explicit metadata and capture the ID printed by `add`:

```bash
pinto add "Implement the Markdown parser" --points 3 --label backend
pinto add "Add parser fixtures" --parent T-1 --depends-on T-2 --body "Cover valid and invalid input."
```

Use `--template <name>` only after confirming that `.pinto/templates/item/<name>.md` exists. Prefer `--body` and field options in non-interactive sessions; `add --edit` and `edit` without field options launch `$VISUAL` or `$EDITOR`.

Update fields, hierarchy, and sibling priority through pinto:

```bash
pinto edit T-1 --title "Implement the Markdown parser" --points 5
pinto edit T-1 --parent T-3
pinto edit T-1 --no-parent
pinto reorder T-1 --before T-2
pinto reorder T-1 --top
```

Treat labels supplied to `edit --label` as a replacement set. Reorder only within the same parent-and-status sibling group; use `edit --parent` to change groups and `move` to change status.

Transition one or more PBIs by placing the destination status last, like Unix `mv`:

```bash
pinto move T-1 in-progress
pinto move T-1 T-2 review
```

Use an exact column from `.pinto/config.toml`. Keep WIP-limit warnings enabled; pass `--no-wip-check` only when the user explicitly wants to bypass them. Verify every write with `show`, `list`, or `board` before continuing.

Use `pinto kanban` only when the user wants an interactive TUI and an interactive terminal is available. Use `pinto board` or `pinto board --json` for non-interactive inspection.

Record relations with their dedicated commands:

```bash
pinto dep add T-2 T-1
pinto dep rm T-2 T-1
pinto link add T-1 abc1234
pinto link sync
```

Interpret `pinto dep add T-2 T-1` as “T-2 depends on T-1.” Remember that `link sync` writes the links it discovers; run it only for a requested link update in a Git repository, then review the result.

Archive by default:

```bash
pinto rm T-1
```

Use `pinto rm T-1 --force` only when the user explicitly requests irreversible deletion. Remove active parent or dependency references first if permanent removal is rejected.

## Manage Sprints and completion

Inspect Sprints before creating or changing one, then use explicit IDs and dates:

```bash
pinto sprint list --json
pinto sprint new S-1 "Sprint 1" --goal "Ship the parser" --start 2026-07-01 --end 2026-07-14
pinto sprint add S-1 T-1
pinto sprint add S-1 --status todo --limit 3
pinto sprint start S-1
```

Bulk assignment selects matching PBIs in backlog priority order; omit `--limit` only when assigning every match is intended. Assign work only to planned or active Sprints. Use `pinto sprint unassign S-1 T-1` to correct an assignment.

Advance Sprint states only as `planned` → `active` → `closed`. Set a non-blank goal before `sprint start`. pinto interprets Sprint dates as UTC, including date-only values at `00:00` UTC; convert user-local or relative dates to explicit UTC values before writing them, ask when the intended date or timezone is ambiguous, and pass `--start` and `--end` together. Set both dates before requesting a burndown report.

Close and report only when the requested workflow conditions are satisfied:

```bash
pinto sprint close S-1
pinto sprint burndown S-1 --json
pinto sprint velocity
pinto cycletime --sprint S-1 --json
```

Use `pinto sprint remove S-1` only when removing the Sprint itself is intended; it releases assigned PBIs but does not delete them.

Display or deliberately replace the shared Definition of Done:

```bash
pinto dod
pinto dod set "- [ ] Tests pass and documentation is updated"
pinto dod clear
```

Do not clear or replace the Definition of Done as a side effect of updating one PBI.

## Apply multi-command plans safely

Use `automate` for several related commands that benefit from one reviewable plan. Store argv arrays without the leading `pinto` executable, and keep every argument as a JSON string:

```json
{
  "commands": [
    ["edit", "T-12", "--points", "3"],
    ["move", "T-12", "in-progress"]
  ]
}
```

Preview the complete plan first, then apply the same file only when the preview succeeds and the user requested the writes:

```bash
pinto automate --plan plan.json --dry-run --json
pinto automate --plan plan.json --json
```

Use `--dry-run` to execute the validated commands in an isolated copy of the board without changing the source `.pinto/` data.

Use a file or standard input for long or multiline values. Do not put API keys, provider settings, shell syntax, or unknown fields in the plan. Do not include recursive or interactive commands (`automate`, `shell`, `kanban`, or `completion`). The plan runs through normal CLI validation and never invokes a shell.

Check the report's `status`, `dry_run`, and per-command `status` fields instead of parsing localized error text. Treat a real apply as sequential rather than transactional: a failure leaves earlier commands applied, stops at the failing command, and marks later commands `skipped`. Inspect the applied prefix before repairing or retrying a partial failure.

## Preserve the local source of truth

- Use pinto commands for normal PBI, Sprint, relation, rank, and archive operations.
- Treat `.pinto/` as the authoritative board. Do not maintain a second hand-edited backlog.
- Edit `.pinto/config.toml` directly only for intentional board configuration changes, and keep those changes small and reviewable.
- Reserve direct edits to task files for data recovery with no concurrent pinto process; run `pinto list` afterward to validate the complete board.
- Keep `.pinto/issued_ids`; deleted IDs remain reserved and must not be reused.
- Review board writes with `git diff -- .pinto` when the project uses Git.
