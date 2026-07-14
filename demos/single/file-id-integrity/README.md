# File ID integrity demo

This demo exercises the file backend's filename/frontmatter identity checks with
active tasks, an archived task, and a Sprint. Every read validates the logical ID
and rejects duplicate records before a command can mutate the board.

Run these commands from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-1
cargo run --manifest-path ../../../Cargo.toml -- sprint list
cargo run --manifest-path ../../../Cargo.toml -- add "Allocate another item"
```

The dataset already contains an active item allocated after an archived item;
the last command allocates the next unused ID, proving that `next_id` considers
the validated archive record. If manual recovery is ever needed, repair the filename or
frontmatter and run `pinto list` before retrying writes; duplicate IDs and archive
destination collisions stop before existing data is overwritten.
