# pinto

> ***Clarity, Simplicity and Humanity.***  
> ***With or without AI.***

[![CI](https://github.com/moriturus/pinto/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/moriturus/pinto/actions/workflows/ci.yml) [![Crates.io](https://img.shields.io/crates/v/pinto-cli.svg)](https://crates.io/crates/pinto-cli)

**pinto** is a lightweight, local-first Scrum backlog and Kanban board for the terminal.
It keeps Product Backlog Items (PBIs), Sprints, and board state in readable text files so that every change is easy to inspect with Git.

## Principles

- **Fast and simple.** Minimal dependencies and a small vocabulary.
- **Focused on Scrum.** Product Backlog, Sprints, and Kanban—not project-suite features such as billing, CRM, or Gantt charts.
- **Plain text and Git-friendly.** Board data is Markdown with TOML frontmatter.
- **Local first.** No server, database service, or account is required.

## Installation

Install Rust 1.89 or newer (the minimum supported Rust version; Rust 2024 edition itself requires Rust 1.85 or newer), then build from source:

```bash
git clone https://github.com/moriturus/pinto
cd pinto
cargo install --path . --locked
```

The latest published release is `0.2.0`. Install it from crates.io with:

```bash
cargo install pinto-cli --version 0.2.0
```

The installed binary remains `pinto`.

## Agent skill

The repository includes a `pinto-workflow` Agent Skill for safe, user-facing backlog management and
local-first Scrum workflows. See [docs/skills.md](docs/skills.md) for reproducible installation and
usage with the official `skills` CLI.

## Documentation

The user and contributor guide is built with mdBook. Install the managed tools and build it with:

```bash
mise install
mise run book
```

Run `mdbook serve` to preview the site locally. The source is in
[`docs/book/src`](docs/book/src), and the generated site is written to `target/book/`. The
repository's design, JSON, migration, and dogfooding reference documents remain available under
[`docs/`](docs/).

The published book is available at [moriturus.github.io/pinto](https://moriturus.github.io/pinto/).
The [`pages.yml`](.github/workflows/pages.yml) workflow rebuilds and deploys it to GitHub Pages on
pushes to `main` and on manual dispatch.

## Quick start

```bash
# Create a board in the current directory.
pinto init

# Add a Product Backlog Item (PBI) with multiple labels.
pinto add "Implement the parser" --points 3 --label backend cli

# Inspect and update it.
pinto list
pinto show T-1
pinto move T-1 in-progress
pinto next

# Render the board.
pinto board
```

Example board output:

```text
todo (1)
  T-2  Design the board layout

in-progress (1)
  T-1  Implement the parser

review (0)
  (empty)

done (0)
  (empty)
```

## Commands

| Command | Description |
| --- | --- |
| `pinto init` | Initialize `.pinto/` in the current directory. It is idempotent. |
| `pinto add <title>` | Add a PBI. Use `--label <label>...` to set one or more labels in one occurrence; repeating `--label` is equivalent. Other options include `--points`, `--sprint`, `--body`, `--edit`, and `--template`. |
| `pinto list` | List PBIs; filter with `--status <status>...` (or repeat `--status`), `--sprint <id>`, `--label <label>...` (OR; use `--all-labels` for AND), or `--search/-F` (use `--regex/-R` for regular expressions). Use `--roots-only` to omit PBIs with a parent. `--long/-l` shows `ID`, `TITLE`, `STATUS`, `POINTS`, `ASSIGNEE`, `CREATED`, and `UPDATED`; add `--label`, `--sprint`, or `--acceptance-criteria/-A` to include the corresponding column. |
| `pinto next` | Show up to one highest-ranked unstarted PBI whose dependencies are complete; use `--count/-n`, `--sprint/-S`, or `--json/-j` to adjust the result. |
| `pinto show <id>...` | Show one or more PBIs in input order, including Acceptance Criteria progress; `--plain` keeps raw Markdown and `--json` always returns an array. |
| `pinto edit <id>` | Update PBI fields; `--label <label>...` replaces its labels. With no field, open `$VISUAL`/`$EDITOR`. |
| `pinto rm <id>...` | Archive (default) or permanently delete one or more PBIs. |
| `pinto move <id> <status>` | Move a PBI to a workflow column; moving to `done_column` warns when Acceptance Criteria are incomplete but still succeeds. |
| `pinto reorder <id>` | Reorder a PBI with `--before`, `--after`, `--top`, or `--bottom`. |
| `pinto board` | Display the board (PBI by column). Filter by `--status <status>...` (or repeat `--status`) for multiple columns, Sprint, or `--label <label>...` (OR; use `--all-labels` for AND). Use `--roots-only` to omit PBIs with a parent; `--long/-l` uses the same detail columns as `pinto list --long`, with `--label`, `--sprint`, and `--acceptance-criteria/-A` available as column selectors. |
| `pinto kanban` | Open the interactive terminal board. By default, `[tui].hidden_columns` is omitted; use `--column <status>...` (or repeat `--column`) to override the configured display columns. |
| `pinto dep add/rm` | Add or remove item dependencies. |
| `pinto link add/rm/sync` | Associate Git commits with PBIs, or synchronize links from commit messages containing item IDs. |
| `pinto dod` | View, set, or clear the shared Definition of Done. |
| `pinto sprint` | Create, edit, delete, start, close, list, assign, and report on Sprints (`burndown`, `velocity`, `capacity`). |
| `pinto cycletime` / `pinto ct` | Report cycle and lead-time metrics. |
| `pinto rebalance` | Reassign oversized ranks while preserving item order. Use `--dry-run` to preview changes. |
| `pinto migrate --to <backend>` | Move a board between the file, Git, and optional SQLite storage backends. |
| `pinto automate --plan <JSON/PATH/->` | Validate and execute a structured plan from inline JSON, a file, or standard input; add `--dry-run` or `--json` for safe previews and machine-readable results. |
| `pinto shell` | Start the interactive shell with history, editing, and completion. |
| `pinto completion <shell>` | Print a completion script for a supported shell. |

Run `pinto --help` or `pinto <command> --help` for the complete interface.

## Command examples

Run these commands from a directory initialized with `pinto init`. Replace
example IDs, labels, and dates with values from your board.

### Backlog and board

```bash
pinto init
pinto add "Implement the parser" --points 3 --label backend cli
pinto list --status todo --long
pinto list --status todo --long --acceptance-criteria
pinto list --status todo in-progress --json
pinto list --label backend frontend                 # either label (OR)
pinto list --label backend frontend --all-labels    # both labels (AND)
pinto list --roots-only --status todo --json       # roots only, machine-readable
pinto next                                           # highest-ranked actionable PBI
pinto next -n 3 --sprint S-1 --json                  # several candidates for one Sprint
pinto show T-1
pinto move T-1 in-progress
pinto reorder T-1 --top
pinto edit T-1 --title "Implement the Markdown parser" --label backend cli
pinto rm T-1 T-2             # archive one or more PBIs by default
pinto rm T-1 T-2 --force     # permanently remove several PBIs
pinto board --sort rank
pinto board --roots-only --status todo --long
pinto board --long --acceptance-criteria
pinto kanban --column in-progress
pinto rebalance --dry-run
```

`pinto next` is read-only. It considers PBIs in the first configured workflow column as
unstarted, excludes the configured `done_column`, and returns only items whose declared
dependencies all exist and are in that completion column. Results use the same canonical
backlog order as `list`; a missing dependency keeps an item blocked. `--count` defaults to `1`,
and `--sprint` applies an exact Sprint filter.

### Acceptance Criteria progress

Pinto counts Markdown task-list checkboxes in each PBI body and displays the result as
`completed/total` in `show` and the Kanban details popup. Use `--acceptance-criteria` (or `-A`)
with `list --long` or `board --long` to add the same value as a table column. The count is
computed at read time, so it does not add a frontmatter field or rewrite the body. Moving an item
to the configured `done_column` prints a warning when any counted checkbox remains unchecked; the
move is still successful.

`kanban` opens the interactive terminal board. Its footer keeps the five primary
operations visible: cursor movement, expand, details, normal quit, and help. Press
`?` to open the secondary-operation help window; `q` opens a quit
confirmation and `e` edits the selected item with your editor. The help window is non-modal:
commands remain available while it is displayed and close it once accepted; press `?` to close it
without running another command.

To hide workflow columns from the default Kanban display, add their exact names under `[tui]`.
The workflow order remains the order in `columns`, and `kanban --column` overrides this setting
for one invocation:

```toml
[tui]
hidden_columns = ["backlog"]
```

For example, with `columns = ["backlog", "ready", "in-progress", "review", "done"]`, the
default display contains `ready`, `in-progress`, `review`, and `done`. Unknown hidden columns are
reported as configuration errors. `--column ready in-progress` displays only those columns, while
still retaining the full workflow for Kanban movement operations.

Human-readable timestamps use the operating system's local timezone by default. Set
`[display].timezone` in `.pinto/config.toml` to `local`, `UTC`, or a fixed offset such as
`+09:00` or `-05:00`:

```toml
[display]
markdown = true
timezone = "+09:00"
```

This setting applies to timestamp columns in `list --long` and `board --long`, Sprint
periods in `sprint list`, PBI details from `show`, and the Kanban details popup. JSON output
always keeps timestamps in UTC RFC3339 form for stable machine-readable data. Unsupported
timezone values are rejected with the accepted formats in the error message.

Parent PBI points are opt-in. Set `[points].aggregate_children` to `true` to derive a parent
PBI's displayed points from its active descendant leaves:

```toml
[points]
aggregate_children = true
```

The parent's stored points are ignored while it has children, and nested parent values are counted
only once through their leaves. PBIs in `done_column` do not contribute. An active descendant leaf
without points makes the affected parent uncomputed (`-`); this avoids presenting an incomplete
sum as a complete estimate. The default is `false`, so existing boards keep their stored points.

The configuration schema is strict. Unknown keys (for example, a misspelled
`[display].timezome`) are rejected with their table and field path instead of
silently using a default. Workflow columns must be non-blank and unique, and
`done_column`, hidden columns, and WIP limit keys must name configured columns.
The project name must not be blank, and the project key uses the same
ASCII-letter grammar as PBI IDs.

### Dependencies, Git links, and Definition of Done

```bash
pinto dep add T-2 T-1        # T-2 depends on T-1
pinto dep rm T-2 T-1
pinto link add T-1 abc1234
pinto link rm T-1 abc1234
pinto link sync
pinto dod                     # print the shared DoD
pinto dod set "Tests pass and documentation is updated"
pinto dod clear
```

### Sprints and reports

```bash
pinto sprint new S-1 "Sprint 1" --goal "Ship the parser" --start 2026-07-01 --end 2026-07-14
pinto sprint edit S-1 --goal "Ship the parser" --start 2026-07-01 --end 2026-07-14
pinto sprint start S-1
pinto sprint add S-1 T-1
pinto sprint add S-1 --status todo --limit 3  # assign the top 3 matching PBIs
pinto sprint add S-1 --status todo             # assign all matching PBIs
pinto sprint unassign S-1 T-1
pinto sprint remove S-1       # releases assigned PBIs without deleting them
pinto sprint list
pinto sprint capacity S-1 --daily-hours 8 --holidays 0 --deduction-factor 0.2
pinto sprint burndown S-1
pinto sprint velocity
pinto cycletime --sprint S-1
```

Close a completed sprint with `pinto sprint close S-1`. When unfinished work remains, choose
`pinto sprint close S-1 --rollover S-2` to move it to a planned or active Sprint, or
`pinto sprint close S-1 --release` to clear its Sprint assignment. The options are mutually
exclusive; omitting both keeps the existing assignments. Completed PBIs are never reassigned or
rewritten by close.

Velocity points, averages, and changes include only work completed no later than the Sprint's
actual close time. Close-time unfinished points and counts appear separately as `spillover`, so
later completion cannot inflate a past Sprint's velocity. The `burndown`,
`velocity`, and `cycletime` reports can also emit JSON with `--json` where
available, making them easy to use in scripts.

### Storage, automation, and shell integration

```bash
pinto migrate --to git
pinto automate --plan '{"commands":[["add","Draft release notes"]]}'
pinto automate --plan plan.json --dry-run --json
pinto automate --plan - --json < plan.json
pinto shell
pinto completion zsh > "${fpath[1]}/_pinto"
```

The Git backend creates one commit for each complete write operation (a
multi-file migration is one operation). It isolates pre-existing Git changes
with a temporary index, so they remain available for the user's own commit.
If it needs to initialize a Git repository for the first write, pinto prints a
warning first. `automate`
accepts only a validated argv-style JSON plan; it does not execute shell code. Use a file path or
`-` for standard input when a plan contains long or multiline bodies. `--dry-run` validates and
executes the plan in an isolated copy of the board, reporting planned changes without modifying
the real board. The source board is locked while that snapshot is taken. For a normal repository
or a linked worktree, pinto copies only `.pinto` and creates a temporary owner-private Git
repository when the source project has Git metadata; it never recursively copies the source `.git`
object store. The temporary workspace is removed after both successful and failed previews. `--json`
reports each command as `valid`, `succeeded`, `failed`, or `skipped`,
including created and updated item IDs and recovery-relevant errors. An `add` command can combine
`--template default` with `--body` without opening an editor.

## TUI demo

Start the interactive board with `pinto kanban` (or `pinto k`) in a terminal:

```text
┌ todo ─────────────────┬ in-progress ──────────┬ review ────────────────┐
│ T-2 Design board       │ T-1 Implement parser  │ (empty)                │
│                        │                        │                        │
├────────────────────────┴────────────────────────┴────────────────────────┤
│ h/l,j/k cursor  Space expand                                            │
│ v details  q quit                                               ?: help │
└───────────────────────────────────────────────────────────────────────────┘
```

This ASCII preview shows the layout without requiring image assets or a
particular terminal theme. The live TUI adapts columns to the terminal width,
displays the selected item's details, and keeps secondary key bindings in the
`?` help window.

## FAQ

### Where is my board stored?

Everything is local to `.pinto/` beneath the directory where you run `init`.
PBI files are readable Markdown with TOML frontmatter, so ordinary Git tools
can review their history.

### Can I customize the workflow?

Yes. Edit `.pinto/config.toml` to set `columns` and `done_column`; subsequent
`list`, `move`, `board`, and TUI commands use the new workflow immediately.

Kanban key assignments live under `[tui.key_bindings]`. Each operation accepts
an ordered array; every entry is active and the first entry is shown in the
fixed footer or the `?` help window:

```toml
[tui.key_bindings]
details = ["Ctrl+d", "v"]
quit = ["Cmd+q", "Esc"]
help = ["?"]
select_left = ["h", "Left"]
add = ["a"]
dependency_add = ["d"]
dependency_remove = ["D"]
parent = ["p"]
```

Key names include `Esc`, `Enter`, `Space`, `Left`/`Right`/`Up`/`Down`, and
single characters. Modifiers are written as `Ctrl+`, `Alt+`, `Shift+`, `Cmd+`,
`Meta+`, or `Hyper+`; `Cmd` maps to the terminal's platform-specific
Command/Super modifier. For printable keys, write the resulting character
directly (`A` or `<`, not `Shift+a` or `Shift+,`), including after another
modifier (`Ctrl+A`, not `Ctrl+Shift+a`). Shift remains available for named keys
such as `Shift+Left`. Other keys use their key names.
The footer
shows the first configured key for the five primary operations plus help; every
other accepted operation key appears in the `?` help window. The default `?` key
opens help, so the default
regular-expression search key is `Ctrl+?`. Missing operations retain the built-in
defaults, while invalid names report the operation and the supported syntax.

In Kanban, `a` opens a two-step form for a new PBI (title, then body). `d` adds a
dependency, `D` removes one, and `p` sets or clears the selected PBI's parent;
in each relation form, move the cursor to another card and press `Enter`, or type an ID directly.
Press `Enter` on the selected source with an empty parent field to clear its parent.
`Esc` cancels without changing the board. These operations reuse the CLI validation
and persistence services, so invalid IDs, missing items, parent cycles, and dependency
cycle warnings use the same rules as the corresponding CLI commands.

### How do I get command-specific help?

Run `pinto <command> --help`, for example `pinto sprint --help` or
`pinto migrate --help`. This is the authoritative reference for every option.

### Does pinto need a server or account?

No. It is local-first and works with the filesystem and optional local Git
repository already on your machine.

## Board files

`pinto init` creates `.pinto/config.toml` and stores each PBI as a separate
Markdown file. The default workflow is `todo → in-progress → review → done`.
Customize its column names and order in `config.toml`; `pinto board` and
`pinto move` immediately use the updated workflow. The configured `done_column`
is displayed newest-first by completion time.

The same configuration file contains `[tui]` settings, including the complete
default `[tui.key_bindings]` table written by `pinto init`.

`pinto rm` archives one or more PBIs to `.pinto/archive/` by default. Use
`--force` only when permanent deletion is intended.

## Sprints and reports

Create a Sprint with `pinto sprint new`, then assign PBIs with `pinto sprint
add`. A goal-less planned Sprint can be repaired with `pinto sprint edit`
before starting it. `pinto sprint remove` removes the Sprint and releases its
PBIs without deleting them. Assign PBIs only to `planned` or `active` Sprints;
existing assignments remain visible after close and can be removed with
`pinto sprint unassign`. A Sprint progresses from `planned` to `active` to `closed`. Capacity,
velocity, burndown, and cycle-time reports are local calculations over this
stored board data; they require no external service.

## Templates and editor support

Reusable item and Sprint bodies live in these plain-text paths:

```text
.pinto/templates/item/<name>.md
.pinto/templates/sprint/<name>.md
```

Apply them through `--template`. `pinto add --edit` opens an empty temporary
file (or a template) in `$VISUAL`, falling back to `$EDITOR`; its saved contents
become the item body.

## Automation and JSON

Most read commands accept `--json` for machine-readable output. `pinto
automate --plan` accepts a JSON argv plan from inline input, a file, or standard
input and runs each command through the same validation, service, and storage
paths as the normal CLI. It neither stores API keys nor requires a particular AI
provider.

## Development

The project uses [mise](https://mise.jdx.dev) for tool versions and common
tasks:

```bash
mise install
mise run test
mise run lint
mise run fmt
mise run check
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the TDD workflow and
[docs/DESIGN.md](docs/DESIGN.md) for architecture decisions.
Stability and dependency trade-offs are recorded in
[docs/stability.md](docs/stability.md) and [docs/dependencies.md](docs/dependencies.md).

### Reproduce CI locally

The [local CI guide](docs/book/src/local-ci.md) documents `nektos/act` commands
for the host platform. macOS/Linux hosts can run the Linux `release` job without
starting the Windows matrix; a Windows host can select the Windows matrix entry
with `-P windows-latest=-self-hosted`.

## License

Distributed under the [MIT License](LICENSE).
