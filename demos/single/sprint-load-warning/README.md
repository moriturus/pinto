# sprint-load-warning

This board demonstrates the non-blocking warnings emitted when a Sprint's estimated assigned
points exceed its configured capacity or historical velocity comparison.

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint start S-2
cargo run --manifest-path ../../../Cargo.toml -- sprint add S-2 --status todo
```

`S-2` has four assigned points and a three-hour capacity, so `sprint start` demonstrates the
capacity warning. The preceding closed `S-1` completed two points; assigning the two `todo` items
to `S-2` also demonstrates the velocity warning. Both operations succeed and retain their
assignments.
