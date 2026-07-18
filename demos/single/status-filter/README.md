# status-filter (single feature: status and assignee filters)

Dataset for selecting multiple workflow columns with space-separated or repeated `--status` options.
It also includes Sprint, label, and assignee metadata so the filters can be combined.

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --status todo in-progress
cargo run --manifest-path ../../../Cargo.toml -- list --status todo in-progress --sprint S-1 --label backend --json
cargo run --manifest-path ../../../Cargo.toml -- list --assignee alice --json
cargo run --manifest-path ../../../Cargo.toml -- board --status todo in-progress
cargo run --manifest-path ../../../Cargo.toml -- board --status todo in-progress --sprint S-1 --label backend --json
cargo run --manifest-path ../../../Cargo.toml -- board --assignee alice --json
# Repeating the option is equivalent: --status todo --status in-progress
```
