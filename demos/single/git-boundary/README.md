# git-boundary (single feature: Git commit boundaries)

This dataset demonstrates the Git backend's one-commit-per-operation boundary. The repository
starts with the default file backend so it can be migrated into a Git-backed board during the
walkthrough.

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-1 T-2
cargo run --manifest-path ../../../Cargo.toml -- migrate --to git
cargo run --manifest-path ../../../Cargo.toml -- add "Inspect the new operation commit"
git log --oneline -- .pinto
git status --porcelain
```

Before the `add` command, stage an unrelated change with `git add` to see that it remains outside
the pinto commit. The transient `.pinto/.lock` is also removed from the commit tree. A failed Git
commit leaves the durable board change in the worktree so it can be reviewed and recovered with
normal Git commands.
