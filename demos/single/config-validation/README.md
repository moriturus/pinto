# config-validation (single feature: strict configuration validation)

This board contains a valid configuration and one PBI for checking the normal
CLI path. The loader rejects unknown TOML keys and semantic mistakes before a
storage backend is opened.

Run the healthy commands from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long --label configuration
cargo run --manifest-path ../../../Cargo.toml -- board
```

To reproduce a configuration error, temporarily edit `.pinto/config.toml` and
run `board` after each change:

```toml
[display]
timezome = "UTC" # typo: the diagnostic names [display].timezome
```

Other useful invalid states are a blank or duplicate entry in `columns`, a
`[wip.limits]` key that is not a configured column, or a whitespace-only
`[project].name`. Each error exits with code 1 and explains which field to fix.
Restore the valid configuration before using write commands.
