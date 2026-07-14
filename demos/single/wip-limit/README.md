# wip-limit (single feature: WIP limits)

Dataset for WIP-limit warnings. `[wip.limits]` in `config.toml` sets `in-progress = 2` and
`review = 1`; both columns are already at their limits.

```bash
cargo run --manifest-path ../../../Cargo.toml -- board                       # warn on columns at their limits
cargo run --manifest-path ../../../Cargo.toml -- move T-3 in-progress        # move beyond limit (warning)
cargo run --manifest-path ../../../Cargo.toml -- move T-3 in-progress -w     # move with WIP checks disabled
cargo run --manifest-path ../../../Cargo.toml -- move T-5 review             # move beyond review limit (warning)
```
