# board-view (single feature: board display)

Dataset for board display options. Every column contains PBIs, including a long title and a
seeded Sprint.

```bash
cargo run --manifest-path ../../../Cargo.toml -- board                    # display by column
cargo run --manifest-path ../../../Cargo.toml -- board --long             # detailed columns in each board column
cargo run --manifest-path ../../../Cargo.toml -- board --long --label     # include labels without filtering
cargo run --manifest-path ../../../Cargo.toml -- board --long --sprint    # include Sprint without filtering
cargo run --manifest-path ../../../Cargo.toml -- board -s in-progress     # show one column
cargo run --manifest-path ../../../Cargo.toml -- board -S S-1             # Sprint board
cargo run --manifest-path ../../../Cargo.toml -- board -o created         # sort by creation time
cargo run --manifest-path ../../../Cargo.toml -- board -o done -r         # reverse completion-time order
cargo run --manifest-path ../../../Cargo.toml -- board --full             # show full long text
cargo run --manifest-path ../../../Cargo.toml -- board -j                 # JSON output
```
