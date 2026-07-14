# Documentation workflow demo

This demo accompanies the mdBook guide. It provides a small board that can be
used to follow the installation, quick-start, CLI, and dogfooding examples
without changing the repository's self-hosted backlog.

Run the commands from this directory so the demo's `.pinto/` board is used:

```bash
cargo run --manifest-path ../../../Cargo.toml -- init
cargo run --manifest-path ../../../Cargo.toml -- add "Publish the project guide" --label docs --body "Build and review the mdBook site."
cargo run --manifest-path ../../../Cargo.toml -- add "Review the CLI examples" --label docs
cargo run --manifest-path ../../../Cargo.toml -- add "Review public Rustdoc wording" --label docs --body "Run cargo doc with warnings denied and review public API comments."
cargo run --manifest-path ../../../Cargo.toml -- add "Review contributor guidance" --label docs --body "Check AGENTS.md, contributor docs, and GitHub templates for consistent English references."
cargo run --manifest-path ../../../Cargo.toml -- move T-1 done
cargo run --manifest-path ../../../Cargo.toml -- board
```

The generated board data is intentionally plain text. Use pinto commands to
modify it, then inspect the result with `list`, `show`, or `board` as described
in the [documentation guide](../../../docs/book/src/introduction.md).

For the public-documentation quality check, run the project gate from the
repository root:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
mise run check
```

The third and fourth sample items record documentation reviews in the demo
board, covering contributor-facing Rustdoc, implementation comments, and
development guidance.
