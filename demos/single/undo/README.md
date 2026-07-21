# undo (single feature: revert the most recent board mutation)

`pinto undo` reverts the most recent completed board mutation. It is a guided,
one-level recovery for a mistaken `move`, `edit`, or `rm --force`, and it works
only on the **git backend**, where each mutation is recorded as a
`pinto: <verb> <id>` commit. This demo ships a two-item board already configured
with `backend = "git"`.

## Run it in a standalone copy

The mutation and undo commands write Git history, so run them in a fresh
directory of their own — not inside this repository's working tree. Copy the
demo board out and initialize a Git repository around it:

```bash
cp -R . /tmp/pinto-undo-demo && cd /tmp/pinto-undo-demo
git init
```

The board is already selected for Git:

```toml
[storage]
backend = "git"
```

## Record and revert a mutation

The first Git-backed write auto-initializes history. Add one more task, confirm
it landed as a commit, then undo it:

```bash
cargo run --manifest-path ../../../Cargo.toml -- migrate --to git
cargo run --manifest-path ../../../Cargo.toml -- add "A mistaken task"
cargo run --manifest-path ../../../Cargo.toml -- undo
# Reverted the most recent board mutation: pinto: add T-3
```

`undo` creates a new commit that reverses the last one — it never rewrites
history — so the undo itself is reviewable and reversible:

```bash
git log --format='%s'
# Revert "pinto: add T-3"
# pinto: add T-3
# ...
```

`T-3` is gone from the board while `T-1` and `T-2` remain.

## When undo refuses

If the most recent commit was not made by pinto (for example a hand-made commit
stacked on top of the board), undo refuses and points you at
`git log -- .pinto` so you can revert the right commit by hand.

On the historyless backends (`file`, `sqlite`) there is nothing to revert, so
`pinto undo` fails with an actionable message and explains the recovery options:
restore from a backup or version-control checkout, or switch to
`backend = "git"` to enable undo for future mutations. The full contract lives in
[Undoing a mutation](../../../docs/book/src/undo.md).
