# parent-child (single feature: hierarchy)

Dataset for epic and child PBI relationships. One epic has three children, and
a second epic has one child.

```bash
cargo run --manifest-path ../../../Cargo.toml -- show T-1         # list children
cargo run --manifest-path ../../../Cargo.toml -- show T-2         # show the parent
cargo run --manifest-path ../../../Cargo.toml -- edit T-6 -P T-1  # change the parent
cargo run --manifest-path ../../../Cargo.toml -- edit T-6 -N      # remove the parent
```

To display only root PBIs, use `--roots-only` with either read command:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --roots-only
cargo run --manifest-path ../../../Cargo.toml -- list --roots-only --json
cargo run --manifest-path ../../../Cargo.toml -- board --roots-only --long
```

The root-only views show the two epics, while their children are omitted. The
filter uses the stored parent link, so a child remains omitted even when its
parent is outside another filter's result.
