# import (single feature: restore a board from a snapshot)

`import` is the inverse of `export --json`. It rebuilds a board's PBIs,
Sprints, Sprint Retros, Sprint Reviews, configuration, and shared Definition of
Done from an export snapshot,
so a board can be restored from a backup or migrated in from another tool
without hand-written automation plans.

This demo ships `snapshot.json`, an `export --json` document, and a `.pinto`
board that was materialized by importing it.

The checked-in snapshot intentionally omits the newer `retros` and `reviews`
collections, so this fixture also demonstrates that older snapshots still
import successfully. Use `demos/single/export` for a snapshot containing both
Sprint child-record types.

## Restore into a fresh board

Initialize an empty board and import the snapshot from a file:

```bash
cargo run --manifest-path ../../../Cargo.toml -- init
cargo run --manifest-path ../../../Cargo.toml -- import snapshot.json
```

The snapshot can also be streamed from standard input with `-`:

```bash
cat snapshot.json | cargo run --manifest-path ../../../Cargo.toml -- import -
```

## Round-trip guarantee

`export --json` followed by `import` reproduces an equivalent board, including
child-record IDs, parent Sprint references, timestamps, and Markdown bodies.
Re-exporting the imported board yields the same JSON document that was imported:

```bash
cargo run --manifest-path ../../../Cargo.toml -- export --json
```

## Replacing an existing board

Importing into a board that already holds PBIs or Sprints fails fast so an
accidental restore never silently overwrites work. Pass `--force` to opt into
replacing the current data with the snapshot:

```bash
cargo run --manifest-path ../../../Cargo.toml -- import --force snapshot.json
```

`import` never runs inside an `automate` plan; it is a manual restore step.
