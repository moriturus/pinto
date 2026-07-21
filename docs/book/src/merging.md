# Merging shared boards

pinto stores each board under `.pinto/` as plain text so it travels through Git
like the rest of the repository. When two people (or two clones, or two
branches) edit the same board in parallel, Git merges most changes cleanly. The
one place that needs a runbook is **new PBIs**, because pinto hands out
sequential IDs and two branches that start from the same commit can allocate the
same number.

This chapter explains why those conflicts appear, how to resolve them without
losing history, and how to confirm the merged board is healthy with
[`pinto doctor`](cli.md).

A ready-to-run board that reproduces the whole scenario lives in
[`demos/single/merge-conflict`](https://github.com/moriturus/pinto/tree/main/demos/single/merge-conflict);
its `README.md` walks through the same steps against a disposable clone.

## Why parallel clones collide

pinto allocates the next ID by reading `.pinto/issued_ids`, an append-only list
of every number it has ever issued:

```text
T-1
T-2
T-3
```

Suppose two branches, `alice` and `bob`, both start from a commit whose latest
item is `T-1`:

- `alice` runs `pinto add` twice and allocates `T-2` and `T-3`.
- `bob` runs `pinto add` once and allocates `T-2`.

Both branches independently decided that the next free number was `T-2`, so
merging them surfaces two kinds of conflict:

- **Task-file conflict** — both branches created `.pinto/tasks/T-2.md` with
  different content, so Git reports an `add/add` conflict on that path.
- **`issued_ids` conflict** — because the branches appended a different number
  of lines, Git reports a content conflict in `.pinto/issued_ids`.

```console
$ git merge alice
Auto-merging .pinto/issued_ids
CONFLICT (content): Merge conflict in .pinto/issued_ids
Auto-merging .pinto/tasks/T-2.md
CONFLICT (add/add): Merge conflict in .pinto/tasks/T-2.md
Automatic merge failed; fix conflicts and then commit the result.
```

`.pinto/tasks/T-3.md` (Alice's second item) merges cleanly because only one
branch created it.

## Resolve `issued_ids` by taking the union

`issued_ids` is a history, not a count: a permanently deleted ID must never be
reissued to a different PBI. The correct resolution is therefore always the
**union** of both sides — keep every number that either branch issued, sorted
and deduplicated. Replace the conflict markers:

```text
T-1
T-2
<<<<<<< HEAD
=======
T-3
>>>>>>> alice
```

with the union:

```text
T-1
T-2
T-3
```

Then stage the file:

```console
$ git add .pinto/issued_ids
```

## Resolve the task-file conflict by re-homing one item

Two different items now claim `T-2`. Pick which one keeps the shared ID, resolve
the file to that item's content, and stage the rest of the merge:

```console
$ git checkout --theirs .pinto/tasks/T-2.md   # keep Alice's item as T-2
$ git add .pinto/tasks/T-2.md .pinto/tasks/T-3.md
$ git commit
```

Re-home the displaced item under a fresh ID with `pinto add`. Because you already
unioned `issued_ids`, `pinto add` allocates the next free number beyond it
(`T-4` here) and appends it to the history:

```console
$ pinto add "Bob X"
$ pinto list
T-1  todo  Baseline
T-2  todo  Alice A
T-3  todo  Alice B
T-4  todo  Bob X
```

Prefer this `pinto add` re-homing over hand-editing task files: it keeps
`issued_ids`, the filename, and the frontmatter ID in agreement automatically.

## Verify the merged board with `pinto doctor`

After every merge, run `pinto doctor`. It scans for the exact damage a bad merge
leaves behind — duplicate IDs, filename/ID mismatches, and `issued_ids` history
gaps — and prints an explicit repair direction for each finding:

```console
$ pinto doctor
Board is healthy.
```

If a naive resolution left two files sharing an ID, `doctor` reports a
`duplicate ID` finding for each copy:

```console
$ pinto doctor
Found 2 unresolved board issue(s).
[duplicate ID] .pinto/tasks/T-2-alice.md: item ID T-2 is also present at ...
Repair: run pinto doctor --fix to renumber duplicates, or resolve them manually
```

`pinto doctor --fix` renumbers the collision deterministically: the first copy
(active tasks before archived items, then by path) keeps the shared ID, and each
later copy is re-homed to a fresh ID above every issued number. The fix rewrites
`parent` and `depends_on` references that point at a renumbered copy, appends the
new IDs to `issued_ids`, and leaves the canonical record untouched:

```console
$ pinto doctor --fix
Found 2 unresolved board issue(s).
Fixed: renumbered T-2 as T-5: .pinto/tasks/T-2-alice.md -> .pinto/tasks/T-5.md
[rank anomaly] .pinto/tasks/T-2.md: rank "j" duplicated in status "todo" parent scope ""
Repair: run pinto rebalance affected workflow scope
[rank anomaly] .pinto/tasks/T-5.md: rank "j" duplicated in status "todo" parent scope ""
Repair: run pinto rebalance affected workflow scope
```

Independent clones usually allocate the same rank alongside the same ID, so the
two renumbered copies now share a rank in one scope. `doctor` will not choose
their order for you; run `pinto rebalance` to spread the collision, then re-run
`pinto doctor`:

```console
$ pinto rebalance
Rebalanced 2/3 item(s) (max rank length 1 -> 1).
$ pinto doctor
Board is healthy.
```

Prefer this over hand surgery. If you would rather choose the surviving item
yourself, keep one file and re-home the other with `pinto add` as above, then
re-run `pinto doctor` until it prints `Board is healthy.`

If you accidentally dropped a line from `issued_ids` while resolving the
conflict, `doctor` detects the gap and `pinto doctor --fix` backfills it — it
only records IDs that already belong to existing items and never chooses between
duplicates:

```console
$ pinto doctor
Found 1 unresolved board issue(s).
[issued ID history] .pinto/tasks/T-4.md: item ID T-4 is missing from issued_ids
Repair: append the existing item ID to issued_ids or run pinto doctor --fix
$ pinto doctor --fix
Board is healthy.
Fixed: recorded T-4 in .pinto/issued_ids
```

## Checklist

1. Union `.pinto/issued_ids`; never drop an issued number.
2. Preserve both versions of a conflicting task under distinct filenames.
3. Run `pinto doctor --fix` to renumber duplicates and repair history gaps.
4. Run `pinto rebalance` when the renumbered copies collide on rank, then re-run
   `pinto doctor` until the board is healthy.
5. Confirm the item list with `pinto list` before pushing the merge.
