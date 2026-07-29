# Sprint Context for Retro and Review

This board demonstrates generated parent-Sprint context in Retro and Review
detail views. The context contains the Sprint goal, state, schedule, capacity,
delivery reports, and close-time spillover without changing the authored
Markdown records.

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint retro show S-1
cargo run --manifest-path ../../../Cargo.toml -- sprint review show S-1 --json
cargo run --manifest-path ../../../Cargo.toml -- sprint retro show S-1 --plain
```

`S-1` is closed. Its unfinished five-point item is assigned to `S-2`, while
the generated context still reports the completed work and the close-time
spillover snapshot. The Review also links `T-3`, an ordinary todo PBI promoted
from a Review action; it has no separate Review status.
