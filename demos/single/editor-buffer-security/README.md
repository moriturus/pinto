# editor-buffer-security (single feature: secure `$EDITOR` buffers)

This board is a small fixture for editor-backed commands. The editor buffer is
created with exclusive creation, owner-only Unix permissions, a sanitized slug,
and RAII cleanup. The buffer is removed after both a successful editor session
and an editor failure; invalid edited content is also discarded without
changing the item.

Run the board commands from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --plain
```

The focused tests cover cross-platform buffer behavior and Unix permissions,
pre-existing paths, symlinks, cleanup, and editor failure paths:

```bash
cargo test --manifest-path ../../../Cargo.toml --bin pinto editor
cargo test --manifest-path ../../../Cargo.toml --test cli add_edit_removes_the_buffer
cargo test --manifest-path ../../../Cargo.toml --test cli edit_without_fields_rejects_invalid_content
```
