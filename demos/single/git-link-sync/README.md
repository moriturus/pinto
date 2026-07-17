# git-link-sync (single feature: synchronize Git commit links)

This dataset demonstrates `pinto link sync`, which reads Git commit messages, finds PBI IDs,
and associates matching commits with those PBIs. The command is idempotent: running it again does
not create duplicate links.

Run the following setup once from this demo directory. The nested Git repository keeps the demo's
history separate from the parent pinto repository:

```bash
git init
git config user.email demo@example.com
git config user.name "Pinto Demo"
git add README.md .pinto
git commit -m "feat: implement T-1"
```

Then run the following commands from this demo directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --json
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --json
cargo run --manifest-path ../../../Cargo.toml -- link sync
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --json
cargo run --manifest-path ../../../Cargo.toml -- link sync
```

The first sync reports one newly linked commit, and the second reports that there are no new
commits to link. The Git history contains a commit whose message includes `T-1`.

Run these commands from the demo directory, not the repository root: the current working
directory selects the board and Git history that pinto operates on.
