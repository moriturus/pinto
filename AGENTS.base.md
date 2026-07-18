# Shared contributor and agent guidance

`AGENTS.base.md` is the versioned shared baseline for people working in the
pinto repository. It contains project rules that should stay consistent across
contributors and coding agents. See [`docs/DESIGN.md`](docs/DESIGN.md) for
detailed design decisions.

Personal preferences and machine-specific instructions belong in a local
overlay. They must not be added to this shared file or committed with the
project.

## Local agent overlays

Developers may derive a local `AGENTS.md` overlay from this baseline and then
append instructions for their own tools, editor, or environment:

```text
cp AGENTS.base.md AGENTS.md
```

Keep the shared rules intact when adding local instructions. Root-level
`AGENTS.md` and `CLAUDE.md`, together with `.claude/`, are ignored by Git for
this purpose. Durable project rules belong in `AGENTS.base.md` or the linked
repository documentation so every contributor can use the same reference.

## Project Overview

**pinto** is a **Scrum backlog and Kanban board** operated through the CLI and
TUI. It manages Product Backlog items, Sprints, and Kanban boards without
requiring users to leave the terminal.

## Design Principles (Highest Priority)

These principles take precedence over all other decisions. When in doubt,
choose the lighter and simpler option.

1. **Lightweight, fast, and simple** — fast startup, few dependencies, and a
   low learning cost.
2. **Scrum-focused** — keep the vocabulary limited to Product Backlog, Sprint,
   and Kanban concepts needed to execute Scrum.
3. **Plain text and Git-friendly** — store data in human-readable files whose
   changes can be reviewed with `git diff`.
4. **Local first** — do not require a server, database service, or account.

### Anti-patterns (Do Not Implement)

- Heavy, full-stack feature growth like Jira or Asana.
- Features unrelated to Scrum or agile execution, such as Gantt charts, paid
  time tracking, CRM, or a document-management platform.
- A complex initialization flow that cannot work without configuration.

For every new feature, ask whether it is necessary for Scrum execution and
whether it preserves the lightweight design.

## Technology Stack

- **Toolchain management and task runner**: [`mise`](https://mise.jdx.dev)
  (`mise.toml`)
- **Language**: Rust (**2024 edition**)
- **CLI parser**: [`clap`](https://docs.rs/clap) (derive API)
- **CLI completion and search**: `clap_complete` generates shell completions;
  `regex` powers regular-expression filters and validation.
- **Interactive Kanban TUI**: `ratatui` with its re-exported `crossterm` backend;
  the `kanban` subcommand is shipped and maintained.
- **Interactive shell**: `rustyline` for line editing, history, and completion.
- **Serialization**: `serde` + `serde_json` + TOML frontmatter (items) / TOML
  (configuration)
- **Markdown rendering**: `termimad` for human-readable PBI and TUI details.
- **Dates and times**: `chrono` (`DateTime<Utc>`, RFC3339)
- **Errors**: `thiserror` in library layers / `anyhow` in the binary
- **Localization**: `fluent-bundle` and `unic-langid` for localized CLI/TUI text.
- **Concurrency**: asynchronous I/O with [`tokio`] / CPU parallelism with
  [`rayon`]
- **Storage and locking**: asynchronous file operations with `tokio`, advisory
  locks with `fs4`, secure editor buffers with `tempfile`, and optional SQLite
  support through bundled `rusqlite`.
- **Terminal layout**: `terminal_size` for terminal dimensions and
  `unicode-width` for display width.
- **Platform boundaries**: Unix PTY tests use `libc`; Windows-specific file
  identity support uses `windows-sys` only on Windows.
- **Testing**: standard tests + `assert_cmd` / `predicates` / `tempfile` for
  CLI integration tests

Keep dependencies to a minimum. Before adding a crate, check whether the
standard library or an existing dependency is sufficient.

### Direct dependency roles

Use `tokio` for waiting on files and processes, `rayon` for CPU-bound
aggregation, `fs4` for the board lock, and `tempfile` for the owner-private
editor buffer. `ratatui` supplies the TUI and its terminal backend, while
`termimad` supplies the shared Markdown rendering used by `show` and the
details popup. `rusqlite` is optional and only enables the SQLite backend; the
default file backend must remain usable without it.

### Choosing Between Async and Parallel Work

Follow the rule **“wait asynchronously, compute in parallel”** (see
[`docs/DESIGN.md`](docs/DESIGN.md) §3.4).

- **I/O-bound work (waiting for files, networks, or databases) → `tokio`**.
  Use `tokio::fs` throughout the persistence layer, parallelize multiple
  resources with `JoinSet`, and do not mix in synchronous blocking I/O.
- **CPU-bound work (analyzing or aggregating many PBIs) → `rayon`**. Split
  calculations across cores with parallel iterators.
- **At the boundary, “collect with async, solve with rayon.”** For example,
  `list` reads resources concurrently and parses them in parallel.
- Do not add a sequential fallback based on item count. Keep I/O on the async
  path in anticipation of more remote I/O.
- Use a multi-threaded Tokio runtime and let Rayon's global pool handle CPU
  parallelism independently.

## Development Workflow (TDD Required)

This project follows **TDD**. Keep the Red → Green → Refactor cycle:

1. **Red**: write a failing test first.
2. **Green**: write the smallest implementation that makes it pass.
3. **Refactor**: improve structure and remove duplication while tests remain
   green.

- Do not write the implementation before the test.
- Test behavior in the single `pinto` crate's domain modules (`backlog`,
  `sprint`, and `rank`), and verify CLI input and output with integration tests.
- A commit should normally contain the test and the implementation that makes
  it pass.

## Common Commands

Use **mise** for toolchain installation and project tasks. Start by running
`mise install` to install the tools declared in `mise.toml`.

```bash
mise install            # Install the tools declared in mise.toml
mise run test           # Run all tests, including all features
mise run lint           # Run Clippy with warnings denied
mise run fmt            # Format Rust sources
mise run book           # Build the mdBook documentation
mise run check          # test + lint + Rust/mdBook docs + fmt --check
mise run coverage       # Measure the Cobertura line-coverage threshold
```

The task definitions in `mise.toml` wrap locked all-feature tests, Clippy, Rust
documentation, `mdbook build`, formatting, and the coverage check. Direct
`cargo` commands are allowed, but prefer `mise run` to keep local and CI
behavior aligned. Public API examples and the CLI PTY tests can be run with:

```bash
cargo test --doc --locked
cargo test --test cli --locked
```

The parser fuzz targets are under `fuzz/`. With nightly Rust and
`cargo-fuzz` installed, list and run them with:

```bash
cargo check --manifest-path fuzz/Cargo.toml --bins --locked
cargo fuzz list
cargo fuzz run automation_plan_parse -- -max_total_time=300
cargo fuzz run markdown_frontmatter_parse -- -max_total_time=300
```

The scheduled CI fuzz job runs both targets and uploads failures from
`fuzz/artifacts`. See [`testing.md`](docs/book/src/testing.md) for the full
reproduction workflow.

Before committing, confirm that `mise run check` passes. It runs tests, lint,
Rust API documentation, mdBook documentation, and formatting checks.

## Backlog Source of Truth (Important)

**pinto self-hosts its own backlog. The sole source of truth is [`.pinto/`](.pinto/).**

- Add, transition, rank, and remove items through pinto commands such as `add`,
  `move`, `edit`, and `rm`. If manual recovery is necessary, edit
  `.pinto/tasks/*.md` directly.
- The former repository-level backlog is not maintained; see
  [`docs/migration.md`](docs/migration.md) for the migration procedure and
  historical background.
- See [`docs/DOGFOODING.md`](docs/DOGFOODING.md) for the specific techniques
  used to validate changes with pinto and update `.pinto/`.

## Coding Conventions

- Keep domain logic independent of the CLI and TUI so it remains easy to unit
  test.
- Avoid `unwrap()` and `expect()` on production code paths; propagate errors as
  `Result` values.
- Add concise documentation comments to public APIs and non-obvious logic.
- Keep user-facing messages and help text concise. Errors should explain what
  to fix and how to fix it.
- Also read [`CONTRIBUTING.md`](CONTRIBUTING.md).
