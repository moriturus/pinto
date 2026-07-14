# Contributing to pinto

Thanks for considering a contribution. pinto is deliberately small: every
change should support practical Scrum work while preserving a fast, local-first,
plain-text tool.

Read [AGENTS.base.md](AGENTS.base.md) and [docs/DESIGN.md](docs/DESIGN.md) before making
design decisions.

## Project principles

1. **Lightweight and simple** — fast startup, few dependencies, low learning cost.
2. **Scrum-focused** — Product Backlog, Sprints, and Kanban are the core scope.
3. **Plain text and Git-friendly** — data must stay readable and reviewable.
4. **Local first** — no server, cloud synchronization, or account requirement.

Avoid turning pinto into a general project-management suite. Features unrelated
to Scrum execution do not belong here.

## Language

Use English for all natural-language content: documentation, code comments,
commit messages, pull requests, issues, and UI/CLI fallback text (the default
locale before localization). Localized user-facing messages (e.g. Fluent
`.ftl` files) are the exception and may be written in their target language.

## Set up the development environment

[mise](https://mise.jdx.dev) manages the toolchain and project tasks:

```bash
mise install
```

## TDD is required

Follow Red → Green → Refactor for every behavior change:

1. Write a failing test.
2. Add the smallest implementation that makes it pass.
3. Refactor only while the test suite remains green.

Keep domain behavior unit-testable under `src/`, and cover CLI input/output with
integration tests in `tests/`. A commit should normally contain both the test
and its implementation.

## Before opening a pull request

Run the full gate:

```bash
mise run check
```

This runs tests, Clippy with warnings denied, Rust API documentation, the mdBook
build, and formatting checks. The same command is used in CI. Update any
affected documentation, keep error messages actionable, and avoid `unwrap()` or
`expect()` on production paths.

See [Testing and fuzzing](docs/book/src/testing.md) for the reproducible public
API doctest, CLI/PTTY smoke-test, and weekly libFuzzer workflow.

Toolchain and package reproducibility are documented in
[Reproducible builds and releases](docs/book/src/reproducibility.md). The
development and release toolchain is pinned by `mise.toml`; the committed
`Cargo.lock` must be honored with `--locked` for source installs, checks, and
release packaging.

## Backlog

pinto dogfoods its own board in `.pinto/`; it is the single source of truth
for project work. Inspect it with `pinto list` or `pinto board`. The legacy
`backlog.md` is a frozen migration snapshot.

## Pull request flow

1. Fork the repository and create a focused branch.
2. Implement the change through TDD.
3. Run `mise run check`.
4. Update the relevant documentation.
5. Open a PR following the repository template.

## Code of conduct and license

All participants must follow the [Code of Conduct](CODE_OF_CONDUCT.md). By
contributing, you agree to license your contribution under the [MIT License](LICENSE).
