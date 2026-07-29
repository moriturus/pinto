# Sprint deletion protection

This board demonstrates that removing a Sprint with a matching Retro or Review
is rejected before mutation. The explicit `--delete-records` option removes only
the matching child records together with the Sprint.

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint remove S-1
cargo run --manifest-path ../../../Cargo.toml -- sprint rm S-1 --delete-records
cargo run --manifest-path ../../../Cargo.toml -- sprint list --json
```

The fixture contains an unrelated Sprint (`S-2`) with its own Retro and Review;
the explicit deletion flow leaves those records intact.
