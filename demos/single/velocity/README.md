# Velocity demo

This directory contains a local board with completed Sprints for trying the
velocity report:

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint velocity
```

Use `--recent N` to select how many recent closed Sprints contribute to the
average. The report distinguishes estimated completed work from unestimated or
unfinished items.
