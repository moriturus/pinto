# automation-plan (single feature: safe structured plans)

Dataset for automation plans with a file source, multiline body input, dry-run validation, and
structured results.

```bash
cargo run --manifest-path ../../../Cargo.toml -- automate --plan plan.json --dry-run --json
cargo run --manifest-path ../../../Cargo.toml -- automate --plan plan.json --json
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --json
```

The dry run executes the plan only in an isolated copy of this demo board; the real run applies
the file plan and keeps its multiline body without an editor.
