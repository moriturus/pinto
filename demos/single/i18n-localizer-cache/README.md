# i18n Localizer cache demo

This demo provides a small board for exercising repeated localized rendering.
`pinto` selects `LC_ALL` before `LANG`, builds the process-local `Localizer`
once, and reuses it for subsequent CLI/TUI message lookups in that process.

Run these commands from this directory:

```bash
LC_ALL=en_US.UTF-8 cargo run --manifest-path ../../../Cargo.toml -- list --long
LC_ALL=ja_JP.UTF-8 cargo run --manifest-path ../../../Cargo.toml -- show T-1
LC_ALL=ja_JP.UTF-8 cargo run --manifest-path ../../../Cargo.toml -- board
LC_ALL=ja_JP.UTF-8 cargo run --manifest-path ../../../Cargo.toml -- kanban
```

The Kanban command keeps one process alive while drawing and handling input;
the unit test `current_reuses_one_localizer_for_the_process_lifetime` guards
the cache identity used by those repeated rendering calls.
