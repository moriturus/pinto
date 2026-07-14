# Rank benchmarks

The `rank_bench` example measures the fractional-indexing operations used to
place PBIs between neighboring items. Run it with:

```bash
cargo run --example rank_bench --release
```

Benchmark results are environment-dependent. Compare runs on the same machine
and toolchain, and focus on regressions rather than absolute numbers. The rank
implementation favors predictable ordering and bounded maintenance work; a
rebalance operation shortens only oversized sibling scopes when ranks become
unnecessarily long.
