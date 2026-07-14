# child-points (single feature: opt-in parent point aggregation)

This board demonstrates recursive parent point aggregation. The feature is
disabled by default; edit `.pinto/config.toml` and set
`[points].aggregate_children = true` to enable it.

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --json
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --json
cargo run --manifest-path ../../../Cargo.toml -- board --json
```

With aggregation enabled, the root PBI uses the active leaf estimates (`5 + 2 = 7`):
its saved value is ignored, the nested child value is not counted a second time,
and the completed leaf is excluded. The original stored estimates remain unchanged.
