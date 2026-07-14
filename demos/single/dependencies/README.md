# dependencies (single feature: dependency links)

Dataset for dependencies between PBIs. It contains a serial chain of five
seeded PBIs.

```bash
cargo run --manifest-path ../../../Cargo.toml -- board            # show dependency markers (⊸ / ⊷ / ⊸!)
cargo run --manifest-path ../../../Cargo.toml -- show T-4         # show Depends on / Depended by
cargo run --manifest-path ../../../Cargo.toml -- dep add T-3 T-1  # add a dependency
cargo run --manifest-path ../../../Cargo.toml -- dep rm T-5 T-4   # remove a dependency
```
