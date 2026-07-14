# Parse error classification demo

This demo keeps a healthy board for reproducing hand-edited configuration and
frontmatter failures. `Error::Parse` and `Error::MissingFrontmatter` are
user-fixable diagnostics, so the CLI exits with code 1; unexpected I/O and task
failures remain exit code 2.

Run the healthy commands from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --plain
```

To reproduce the user-error cases, edit and restore the plain-text files in
`.pinto/` between runs:

```text
.pinto/config.toml: replace a value with invalid TOML, or remove a required section
.pinto/tasks/T-1.md: remove the `+++` delimiters, or make its TOML frontmatter invalid
```

Then run the relevant command and verify it reports the file path without a
panic and exits with code 1:

```bash
cargo run --manifest-path ../../../Cargo.toml -- board       # invalid config: 1
cargo run --manifest-path ../../../Cargo.toml -- list        # invalid frontmatter: 1
```

Repair the edited file before using write commands. The unit classification
test and CLI integration tests cover both parse variants, keep I/O/task failures
separate, and preserve exit code 2 for the true I/O case.
