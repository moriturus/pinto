# Agent skills

pinto ships the `pinto-workflow` skill in `skills/pinto-workflow/`. It gives coding agents a concise
workflow for local-first backlog management, CLI dogfooding, and Git-friendly Scrum work. The
directory follows the `SKILL.md` format consumed by the [Vercel skills CLI](https://github.com/vercel-labs/skills)
and other Agent Skills-compatible tools.

## Inspect the skill

Run these commands from the repository root:

```bash
npx skills add . --list
npx skills use . --skill pinto-workflow
```

The first command must list `pinto-workflow`. The second resolves the local source and prints the
generated prompt without installing it.

## Install for Codex

Install a copy into the project-level Codex skills directory:

```bash
npx skills add . --skill pinto-workflow --agent codex --copy --yes
```

The same command works from a published checkout after replacing `.` with the repository URL:

```bash
npx skills add https://github.com/moriturus/pinto \
  --skill pinto-workflow --agent codex --copy --yes
```

Omit `--copy` to use the CLI's recommended symlink method. Use `--global` only when the skill
should be available outside the current project.

## Verify the installed workflow

Use the installed pinto binary for dogfooding and for other repositories; the target project does
not need to contain pinto source code:

```bash
pinto init
pinto list --status todo
pinto show T-1
pinto board
pinto automate --plan plan.json --dry-run --json
```

Initialize or inspect the board instead of reading task files directly:

```bash
pinto list --status todo --long
pinto show T-1
pinto board
```

The board's canonical data is stored in `.pinto/`; the skill keeps backlog changes local and
reviewable with Git.

## Demo

The [skills demo](../demos/single/skills/README.md) contains a small sample board and repeats the
commands above from an isolated directory.
