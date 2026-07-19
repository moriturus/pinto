# maintainer-workflow (single feature: commit and maintainer review guidance)

This board is a small, runnable checklist for the maintainer workflow described
in `CONTRIBUTING.md`, `SECURITY.md`, and `docs/book/src/reproducibility.md`.
It demonstrates how to review acceptance conditions, keep cross-cutting work in
small, green commits, and record the primary-maintainer fallback for destructive,
release-related, or security-related changes.

From this demo directory, initialize and inspect the board through the repository
binary:

```bash
cargo run --manifest-path ../../../Cargo.toml -- init
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --plain
cargo run --manifest-path ../../../Cargo.toml -- board
```

The corresponding repository checks are:

```bash
cargo test --test docs maintainer_workflow_guidance_has_a_stable_policy_contract -- --exact
cargo test --test demos
mise run check
```

Use the pinto commands above to reproduce the board state; the checked-in board
contains examples for small, green commits, acceptance review, maintainer
verification, and the documented primary-maintainer fallback.
