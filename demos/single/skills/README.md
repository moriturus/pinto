# pinto-workflow skill demo

This demo contains a small `.pinto/` board used to exercise the `pinto-workflow` agent skill.
It keeps the sample data isolated from the repository's own backlog.

From this directory, inspect the demo with the installed pinto binary:

```bash
cd demos/single/skills
pinto list --long
pinto show T-1
pinto board
```

From the repository root, verify and use the distributable skill with the official CLI:

```bash
npx skills add . --list
npx skills use . --skill pinto-workflow
npx skills add . --skill pinto-workflow --agent codex --copy --yes
```

The demo data is intentionally disposable. Use the pinto CLI for any changes made while trying
the workflow; do not edit files under `.pinto/` directly.
