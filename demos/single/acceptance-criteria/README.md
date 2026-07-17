# acceptance-criteria (single feature: Acceptance Criteria progress)

This dataset contains one completed PBI with an incomplete Acceptance Criteria checklist and one
todo PBI with all criteria checked. The first item is intentionally in `done` so the non-blocking
move warning and the displayed `1/2` progress can be inspected.

Run these commands from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- show T-1
cargo run --manifest-path ../../../Cargo.toml -- list --long --acceptance-criteria
cargo run --manifest-path ../../../Cargo.toml -- board --long --acceptance-criteria
cargo run --manifest-path ../../../Cargo.toml -- kanban
```

The progress is derived from Markdown task-list items in each PBI body. It is not stored as a
separate frontmatter field, and the unchecked criteria warning does not prevent a move.
