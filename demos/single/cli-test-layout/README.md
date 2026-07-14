# cli-test-layout (single feature: CLI integration test layout)

Small dataset used while reviewing the feature-oriented CLI integration-test layout. It keeps one
fixture in `todo` and one in `in-progress`, so the standard list, show, board, and move commands
can be tried against a clean board.

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-1 T-2
cargo run --manifest-path ../../../Cargo.toml -- board
cargo run --manifest-path ../../../Cargo.toml -- move T-1 review
```
