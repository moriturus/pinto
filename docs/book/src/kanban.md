# Kanban (TUI)

`pinto kanban` opens an interactive board in the terminal. It reads and writes
the same `.pinto/` board as the non-interactive commands, so a move made in the
TUI is immediately visible to `pinto list` and `pinto board`, and vice versa.

```bash
pinto kanban
```

## Start with a focused view

Startup flags narrow what the board shows without changing stored data:

```bash
pinto kanban --column in-progress review   # show only these columns
pinto kanban --maximize --column review     # open maximized on one column
pinto kanban --sprint S-1                    # show cards assigned to one Sprint
pinto kanban --label ui backend             # match either label
pinto kanban --label ui backend --all-labels # require both labels
pinto kanban --search parser                # filter cards by substring
pinto kanban --sprint S-1 --label ui --column in-progress --search '^T-1\d' --regex
                                             # compose all startup filters
```

Explicit `--column` values override the `[tui] hidden_columns` setting for that
run. `--sprint` matches the assigned Sprint ID exactly. `--label` uses the same OR matching as
`pinto board`; add `--all-labels` for AND matching. `--regex` requires `--search`. All startup
filters are read-only and remain active when the TUI reloads after an edit, move, or reorder.

## Navigate and edit

The board separates *selecting* a card from *moving* it: lowercase keys move the
cursor, uppercase (Shift) keys move the selected item. Defaults are:

| Action | Keys |
| --- | --- |
| Select column / row | `h` `j` `k` `l` or arrow keys |
| Move item across columns | `H` / `L` (Shift+Left / Shift+Right) |
| Reorder item within a column | `K` / `J` (Shift+Up / Shift+Down) |
| Expand or collapse a parent | `Space` / `Enter` |
| Add a PBI | `a` |
| Edit the selected PBI | `e` |
| Add / remove a dependency | `d` / `D` |
| Set or clear the parent | `p` |
| Open the details popup | `v` |
| Substring / regex search | `/` / `Ctrl+?` |
| Clear an active filter | `Esc` |
| Toggle a maximized column | `m` |
| Reload the board | `r` |
| Help window | `?` |
| Quit | `q` or `Esc` |
| Quit into the shell | `Q` |

Press `?` inside the board to open the built-in help window, which always lists
the bindings that are actually in effect.

Cards follow the same hierarchical [display order](cli.md#display-order) as
`pinto list` and `pinto board`: top-level cards by rank, each parent followed by
its subtree, with siblings ordered by rank. Expanding a parent reveals its
children directly beneath it, so a child may sit ahead of a standalone card that
outranks it — that is the point, since the parent's priority carries its whole
subtree. The completion column leads with the most recently finished card
(`done_at` descending).

## Customize behavior

The `[tui]` section of `.pinto/config.toml` adjusts the interactive board:

```toml
[tui]
confirm_quit = true                 # ask before leaving the board
hidden_columns = ["done"]           # hide columns unless --column overrides
```

Unknown column names in `hidden_columns` are rejected at load time, so a typo
surfaces immediately rather than silently hiding nothing.

## Rebind keys

`[tui.key_bindings]` overrides the keys for individual actions. Each action
takes an array of one or more key expressions, and an action may keep several
bindings at once:

```toml
[tui.key_bindings]
quit = ["q", "Esc"]                 # keep the defaults
add = ["a", "n"]                    # add a second key for "add"
move_left = ["Shift+Left"]          # replace the default for this action
help = ["?", "F1"]
```

Only the actions you list are overridden; every other action keeps its default
keys. The action names are the snake_case forms shown by the built-in help
window and the `[tui.key_bindings]` documentation (`quit`, `shell`,
`select_left`, `move_left`, `reorder_up`, `add`, `edit`, `dependency_add`,
`parent`, `maximize`, `search`, `regex_search`, `details`, `help`, and so on).

A key expression is a key name, optionally prefixed with `+`-separated
modifiers:

- Printable keys are the character itself: `q`, `/`, `?`. Use an uppercase
  letter (`H`) rather than `Shift+h` for shifted letters.
- Named keys: `Enter`, `Esc`, `Tab`, `Backspace`, `Delete`, `Insert`, `Home`,
  `End`, `PageUp`, `PageDown`, the arrows `Left` / `Right` / `Up` / `Down`, and
  function keys `F1`–`F12`.
- Modifiers: `Ctrl`, `Alt`, `Shift`, `Cmd`, `Meta`, and `Hyper` — for example
  `Ctrl+a` or `Alt+Shift+Left`. Write the literal plus key as `Plus`.

Invalid expressions (an empty name, an unknown modifier, or `Shift+` on a
printable character) are reported when the board configuration loads, so a bad
binding is caught before the TUI starts rather than failing silently.
