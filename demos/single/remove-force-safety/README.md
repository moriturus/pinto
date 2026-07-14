# remove-force-safety (single feature: permanent removal safety)

This demo contains a target PBI with both a parent reference and a dependency
reference, plus a historical commit link. Permanent removal is rejected while
other PBIs still refer to the target; once those links are cleared, deleting
the target succeeds and the next PBI receives the next unused ID instead of
reusing the deleted one.

Run the commands from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --json
cargo run --manifest-path ../../../Cargo.toml -- rm T-1 --force # rejected; reports T-2 and T-3
cargo run --manifest-path ../../../Cargo.toml -- edit T-2 --no-parent
cargo run --manifest-path ../../../Cargo.toml -- dep rm T-3 T-1
cargo run --manifest-path ../../../Cargo.toml -- rm T-1 --force
cargo run --manifest-path ../../../Cargo.toml -- add "Replacement" # assigns T-4
```

The force-delete operation does not rewrite references automatically. The
generated `.pinto/issued_ids` file is the plain-text history that keeps deleted
IDs reserved.
