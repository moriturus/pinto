# Sprint Review record

This board demonstrates a first-class Sprint Review stored separately from the
Sprint file. The Review uses the Sprint ID as both its ID and filename.

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint review list --json
cargo run --manifest-path ../../../Cargo.toml -- sprint review show S-1 --json
cargo run --manifest-path ../../../Cargo.toml -- sprint review edit S-1 --body "Updated notes"
```

The `--json` commands return an array with the Review ID, Markdown body, and
creation/update timestamps. The parent Sprint is active, demonstrating that a
Review is independent of the Sprint's planned, active, or closed state.
