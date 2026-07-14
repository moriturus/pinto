# remove-multiple (single feature: remove multiple PBIs)

Dataset for removing several PBIs in one command. The board starts with three active PBIs so
both archive-by-default and `--force` can be tried independently.

```bash
cargo run --manifest-path ../../../Cargo.toml -- list
cargo run --manifest-path ../../../Cargo.toml -- rm T-1 T-2       # archive both
cargo run --manifest-path ../../../Cargo.toml -- rm T-3 --force  # permanently delete one
cargo run --manifest-path ../../../Cargo.toml -- rm T-404 T-1     # report the missing ID and continue
```

When any requested ID is missing, pinto reports the error and still attempts every other ID. The
command exits with status 1 if at least one ID failed.
