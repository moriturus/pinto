# pinto

> ***Clarity, Simplicity and Humanity.***  
> ***With or without AI.***

[![CI](https://github.com/moriturus/pinto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/moriturus/pinto/actions/workflows/ci.yml) [![Crates.io](https://img.shields.io/crates/v/pinto-cli.svg)](https://crates.io/crates/pinto-cli)

**pinto** is a lightweight, local-first Scrum backlog and Kanban board for the
terminal. It keeps Product Backlog Items (PBIs), Sprints, and workflow state in
plain text so that every change is easy to inspect and recover with Git.

## Principles

- **Lightweight and focused.** pinto keeps a small Scrum vocabulary instead of
  becoming a general project-management suite.
- **Plain text and Git-friendly.** The file and Git backends keep board data
  readable; SQLite is an optional normalized local backend.
- **Local first.** No server, database service, or account is required.

## Installation

The latest published release is `0.4.3`. Install it from crates.io:

```bash
cargo install pinto-cli --version 0.4.3
pinto --version
```

To install from a checkout, use the committed lockfile:

```bash
git clone https://github.com/moriturus/pinto
cd pinto
cargo install --path . --locked
pinto --version
```

## Quick start

Run these commands from the directory that should own the board:

```bash
pinto init
pinto add "Implement the Markdown parser" --points 3 --label backend
pinto list
pinto move T-1 in-progress
pinto board
```

For the complete first-workflow walkthrough, see [Quick start](docs/book/src/quickstart.md).

## Documentation

The [published Book](https://moriturus.github.io/pinto/) is the primary user
and contributor guide. Build it locally with `mise install && mise run book` or
preview it with `mdbook serve`.

### User guide

- [Introduction](docs/book/src/introduction.md)
- [Installation](docs/book/src/installation.md)
- [Quick start](docs/book/src/quickstart.md)
- [CLI reference](docs/book/src/cli.md)
- [Configuration](docs/book/src/configuration.md)
- [Data format](docs/book/src/data-format.md)
- [Kanban (TUI)](docs/book/src/kanban.md)
- [Cookbook](docs/book/src/cookbook.md)
- [Team-scale best practices](docs/book/src/team-scale.md)
- [Contributing](docs/book/src/contributing.md)
- [Testing and fuzzing](docs/book/src/testing.md)
- [Local CI](docs/book/src/local-ci.md)
- [Reproducibility](docs/book/src/reproducibility.md)
- [Undoing a mutation](docs/book/src/undo.md)
- [Merging shared boards](docs/book/src/merging.md)
- [Dogfooding](docs/book/src/dogfooding.md)

### Repository references

- [Design decisions](docs/DESIGN.md)
- [JSON output](docs/json-schema.md)
- [Storage migration](docs/migration.md)
- [Stability decisions](docs/stability.md)
- [Dependency decisions](docs/dependencies.md)
- [External command contract](docs/plugin-contract.md)
- [Agent skills](docs/skills.md)
- [Benchmarks](docs/benchmarks.md)

The published Book is deployed from `main`, `develop`, and release tags by
[`pages.yml`](.github/workflows/pages.yml). The site root redirects to
`/latest/`; the development Book is under `/develop/`, and versioned Books are
available under `/<version>/`.

## Development

Read [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.base.md](AGENTS.base.md)
before changing the project. The standard local gate is:

```bash
mise install
mise run check
```

See [Reproduce CI locally](docs/book/src/local-ci.md) for platform-specific
`act` commands.

## License

Released under the [MIT License](LICENSE).
