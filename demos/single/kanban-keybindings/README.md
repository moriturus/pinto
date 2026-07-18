# kanban-keybindings (single feature: configurable Kanban keys)

This demo contains two PBIs and a Kanban configuration with custom aliases:

- `A` and `v` open or close details; the footer displays the first as `A`.
- `Cmd+q` and `Esc` leave Kanban mode.
- `?` opens the non-modal secondary-operation help window; `Ctrl+?` starts regex search.
- `a`/`Left` and `d`/`Right` select adjacent columns.
- In the default keymap, `a` opens the add form, `d` adds a dependency, and
  `D` removes one, while `p` sets or clears a parent; this demo overrides
  `a`/`d` for column navigation.
- While adding or removing a relation, move to the target card and press `Enter`
  instead of typing its ID.

Run the commands from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- board
cargo run --manifest-path ../../../Cargo.toml -- kanban
cargo run --manifest-path ../../../Cargo.toml -- kanban --column todo in-progress
```

The shared board keeps only its workflow and display settings in
`.pinto/config.toml`. Personal keybindings are in
`user-config/pinto/config.toml`; set the XDG directory before running the demo:

```bash
export XDG_CONFIG_HOME="$PWD/user-config"
```

The settings use `[tui.key_bindings]`. Try
reordering the aliases: the first key is shown in the fixed footer or help window
while every listed key remains active. `Ctrl+`, `Alt+`, `Shift+`, `Cmd+`, `Meta+`,
and `Hyper+` modifiers are supported. Write printable results directly (`A` or
`<`, not `Shift+a` or `Shift+,`), including after another modifier (`Ctrl+A`,
not `Ctrl+Shift+a`). Named keys may use Shift, such as `Shift+Left`. To hide a column
from the default display, add for example
`hidden_columns = ["todo"]` under `[tui]`; an explicit `kanban --column ...`
override always takes precedence.
