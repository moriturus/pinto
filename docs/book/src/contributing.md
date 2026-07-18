# Contributing

Read the repository's [AGENTS.base.md](https://github.com/moriturus/pinto/blob/main/AGENTS.base.md),
[CONTRIBUTING.md](https://github.com/moriturus/pinto/blob/main/CONTRIBUTING.md),
and [design guide](https://github.com/moriturus/pinto/blob/main/docs/DESIGN.md)
before making a design decision. The project favors a small, fast, Scrum-focused
tool with plain-text, Git-friendly storage.

For a new command, backend, or report, record the Scrum-related need, why
existing functionality is insufficient, and the dependency, persistence, and
migration/compatibility impact in the issue or pull request. See the [design
decision record](https://github.com/moriturus/pinto/blob/main/docs/DESIGN.md)
and [stability decisions](https://github.com/moriturus/pinto/blob/main/docs/stability.md)
for the storage boundary.

`AGENTS.base.md` is the shared contributor and agent baseline. A developer may
derive a local `AGENTS.md` overlay with `cp AGENTS.base.md AGENTS.md` and append
personal tool or environment instructions there. Root-level `AGENTS.md`,
`CLAUDE.md`, and `.claude/` are ignored and must not be committed; project rules
that everyone needs belong in the tracked baseline or this guide.

## Development loop

Install the managed tools and run the quality gate:

```bash
mise install
mise run check
```

Follow TDD for behavior changes:

1. **Red** — write a focused test that fails for the missing behavior.
2. **Green** — implement the smallest change that makes the test pass.
3. **Refactor** — improve structure while keeping the tests green.

Domain behavior belongs in unit-testable modules under `src/`; CLI input and
output belong in integration tests under `tests/`. Documentation changes should
also build the book locally:

```bash
mise run book
mdbook serve
```

The repeatable unit, integration, doctest, and fuzzing commands are collected
in [Testing and fuzzing](testing.md).

See [Reproducible builds and releases](reproducibility.md) for the pinned
toolchain policy, CI job responsibilities, and locked package verification.

## Before committing

Run `mise run check` after the final change. It runs all-feature tests, Clippy
with warnings denied, Rust documentation with warnings denied, the mdBook
build, and formatting checks. Review the complete diff for unrelated changes,
keep dependencies minimal, and write actionable user-facing errors.

Backlog changes are part of the normal workflow: inspect and update the
self-hosted `.pinto/` board through pinto commands, then verify the result with
`pinto list` or `pinto board`.

## Pull requests

Use a focused branch and describe the motivation, implementation, tests, and
documentation changes. Include a related issue or planning reference when one exists.
Follow the pull request checklist and keep user-facing documentation in
English; localized Fluent resources are the intentional exception.
