---
name: pinto-workflow
description: Manage pinto Scrum backlogs and Kanban workflows with local-first, Git-friendly CLI operations. Use when planning or implementing work in a pinto repository or another local board, inspecting the backlog, validating pinto commands, or updating a PBI's status.
---

# Pinto Workflow

Use pinto as a lightweight, local-first Scrum backlog and Kanban board for managing Product Backlog Items (PBIs), Sprints, and board state from the terminal. Keep the workflow lightweight, local-first, and reviewable in Git; do not introduce server-side project-management machinery.

## Repository workflow

Use the installed pinto binary for all board operations. The same workflow works in a pinto source checkout and, outside a pinto checkout, in any other repository that stores its own local board.

If the working directory is a new board, initialize it before inspecting or changing PBIs:

```bash
pinto init
```

1. Inspect the todo column and the selected PBI:

   ```bash
   pinto list --status todo --long
   pinto show T-1
   pinto dod
   ```

   Treat the highest-ranked todo item as the next candidate unless the user specifies another item. Read the PBI's acceptance criteria in `pinto show`, and the shared Definition of Done from `pinto dod`, before changing code.

2. Make the implementation and tests in the repository. Follow the repository's TDD and review instructions.

3. Perform CLI dogfooding against the current worktree and inspect behavior through pinto commands:

   ```bash
   pinto list --status todo
   pinto show T-1 --json
   pinto board
   pinto list --label review
   ```

   The canonical board data lives under `.pinto/`. Use pinto commands for backlog reads and writes; do not edit task Markdown files directly. Keep `.pinto/config.toml` changes explicit and reviewable.

   For several related CLI writes, validate the structured plan without changing the board first:

   ```bash
   pinto automate --plan plan.json --dry-run --json
   ```

4. After the implementation and all required checks pass, update the PBI through the CLI:

   ```bash
   pinto move T-1 done
   pinto show T-1
   ```

   For an interactive review of the whole board, `pinto kanban` opens the same board in a TUI. To confirm flow-level effects of the change, `pinto cycletime` and `pinto sprint velocity` summarize completed work.

## External repository workflow

When this skill is installed into another repository, use the installed pinto binary against that repository's local board:

```bash
pinto init
pinto list --status todo
pinto show T-1
pinto board
pinto automate --plan plan.json --dry-run --json
```

Keep the board local to the current project. The skill does not require the other repository to contain pinto source code.

## Common pinto operations

Use the installed binary when working in a user's initialized board:

```bash
pinto init
pinto add "Implement the parser" --points 3 --label backend
pinto list --status todo --long
pinto show T-1
pinto edit T-1 --title "Implement the Markdown parser"
pinto move T-1 in-progress
pinto reorder T-1 --top
pinto board
pinto rm T-1
```

Use relation, Definition of Done, Sprint, and automation commands when the work needs them:

```bash
pinto dep add T-2 T-1
pinto link scan
pinto dod set "- [ ] Tests pass and docs updated"
pinto sprint list
pinto automate --plan plan.json --dry-run --json
```

Keep command input explicit and use `--dry-run` before applying a multi-write automation plan. Prefer `--json` when another tool needs stable machine-readable output.

## Installing and using this skill

The official `skills` CLI discovers this folder as `skills/pinto-workflow/SKILL.md`. From the repository root, list, install, or use it with:

```bash
npx skills add . --list
npx skills add . --skill pinto-workflow --agent codex --copy --yes
npx skills use . --skill pinto-workflow
```

For a published checkout, replace `.` with the repository URL. Keep the skill name quoted if a source contains spaces; use `--agent codex` to target Codex explicitly.

## Guardrails

- Keep PBI, Sprint, and Kanban vocabulary focused on Scrum work.
- Preserve plain-text, Git-friendly storage and local-first operation.
- Never invent a pinto command; check `pinto <command> --help` or `pinto --help` when unsure.
- Never advise users to migrate data to an obsolete project name or to a separate service.
