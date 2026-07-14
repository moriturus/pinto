# tui-lifecycle (single feature: Kanban terminal lifecycle)

This demo keeps one selectable PBI in a Kanban board so the terminal lifecycle can be exercised
from a real terminal. It covers raw mode, the alternate screen, resize redraws, quit, and the
`e` editor handoff.

Run it from this directory:

```bash
EDITOR=true cargo run --manifest-path ../../../Cargo.toml -- kanban
```

While Kanban is open, resize the terminal, press `e` to hand the terminal to `$EDITOR`, and then
press `q` to leave. Set `EDITOR` to an editor such as `vim` to edit the PBI instead of using the
no-op `true` command. `confirm_quit` is disabled in this demo so the quit path is immediate.
