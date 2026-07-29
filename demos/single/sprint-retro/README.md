# Sprint Retro record

This board demonstrates a first-class Sprint Retro stored separately from the
Sprint file. The Retro uses the Sprint ID as both its ID and filename.

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint retro list --json
cargo run --manifest-path ../../../Cargo.toml -- sprint retro show S-1 --json
cargo run --manifest-path ../../../Cargo.toml -- sprint retro edit S-1 --body "Updated notes"
```

The `--json` commands return an array with the Retro ID, Markdown body, and
creation/update timestamps. The board's Sprint is active, so the fixture also
demonstrates that Retros can be created before a Sprint is closed.
