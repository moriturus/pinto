# labels (single feature: labels)

Dataset for labels and label filters. The items use feature, bug, backend, frontend, perf, security,
and chore labels; several seeded PBIs demonstrate items with multiple labels. Label-setting commands
accept multiple values after one `--label`; repeating the option remains equivalent.

```bash
cargo run --manifest-path ../../../Cargo.toml -- list -L security   # filter by security
cargo run --manifest-path ../../../Cargo.toml -- list -L backend    # filter by backend
cargo run --manifest-path ../../../Cargo.toml -- list -L backend security                 # OR: either label
cargo run --manifest-path ../../../Cargo.toml -- list -L backend security --all-labels    # AND: both labels
cargo run --manifest-path ../../../Cargo.toml -- board -L backend security --json
cargo run --manifest-path ../../../Cargo.toml -- list -l            # list labels for each PBI
cargo run --manifest-path ../../../Cargo.toml -- edit T-1 -l feature frontend ux        # replace labels
```
