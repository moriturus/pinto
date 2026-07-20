# merge-conflict (single feature: merging shared boards)

This board is the **resolved** end state of a parallel-clone merge, described in
the [Merging shared boards](../../../docs/book/src/merging.md) runbook. It holds
four items whose IDs (`T-1`..`T-4`) survived an ID collision without any number
being reused. Inspect it read-only from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list
cargo run --manifest-path ../../../Cargo.toml -- doctor
cargo run --manifest-path ../../../Cargo.toml -- show T-4 --plain
```

`T-2` (`Alice A`) kept the shared ID during the merge, and `T-4` (`Bob X`) is the
re-homed item that originally collided on `T-2`. `doctor` reports the board as
healthy because `issued_ids` is the union of every issued number.

## Reproduce the conflict on a disposable clone

Do not run these against a real board — copy this directory somewhere
throwaway first. The steps below recreate the `add/add` and `issued_ids`
conflicts from the runbook:

Have `alice` and `bob` allocate a different number of IDs so that both the task
file and the append-only `issued_ids` history conflict:

```bash
git init && git add -A && git commit -m base
git checkout -b alice
cargo run --manifest-path ../../../Cargo.toml -- add "Alice fifth"
cargo run --manifest-path ../../../Cargo.toml -- add "Alice sixth"
git add -A && git commit -m alice
git checkout main && git checkout -b bob
cargo run --manifest-path ../../../Cargo.toml -- add "Bob fifth"
git add -A && git commit -m bob
git merge alice        # CONFLICT in .pinto/issued_ids and .pinto/tasks/*.md
```

Resolve `issued_ids` to the union of both sides, keep one item per conflicting
task file, re-home the other with `pinto add`, then verify:

```bash
cargo run --manifest-path ../../../Cargo.toml -- doctor
cargo run --manifest-path ../../../Cargo.toml -- doctor --fix   # backfills issued_ids gaps
```

Re-run `doctor` until it prints `Board is healthy.` See the runbook for the full
walkthrough and the reasoning behind the union rule.
