# deep-board (single feature: stack-safe deep traversals)

This board is a single parent/child hierarchy fifteen levels deep, from the
program epic `T-1` down to the leaf story `T-15`. It demonstrates that pinto
walks deep parent chains — for hierarchical ordering, Kanban row layout, and
point aggregation — on an explicit heap stack, so arbitrarily deep boards never
overflow the call stack.

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --json
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --json
cargo run --manifest-path ../../../Cargo.toml -- board
cargo run --manifest-path ../../../Cargo.toml -- doctor
```

Parent point aggregation is enabled (`[points].aggregate_children = true`), so
the leaf estimate rolls all the way up the chain: `show T-1` reports `3` even
though only `T-15` stores an estimate. `list` and `board` render the whole chain
in hierarchical order, each layer nested under its parent, and `doctor` reports a
healthy board because the hierarchy is acyclic.

The regression suite pushes the same traversals to synthetic chains a hundred
thousand levels deep — far beyond what a native recursion could handle — and
also covers deep dependency chains and cyclic input, which `doctor` still detects
and reports deterministically without looping.
