# stale-lock (single feature: cross-platform stale lock recovery)

This board provides a small dataset for checking that lock ownership comes from
the OS file handle rather than the PID text written for diagnostics. It is safe
to run the examples only when no other pinto process is writing this board.

Run the normal board commands from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- show T-1 T-2
```

To simulate a lock left by a terminated process, write an impossible PID and
retry a write. pinto reuses the file after the OS lock is available:

```bash
printf '%s\n' 999999999 > .pinto/.lock
cargo run --manifest-path ../../../Cargo.toml -- add "Recovered stale lock"
```

To simulate PID reuse, put the current shell PID in the marker without taking
the OS lock. The write still succeeds because marker text is not ownership:

```bash
printf '%s\n' "$$" > .pinto/.lock
cargo run --manifest-path ../../../Cargo.toml -- add "PID text is diagnostic"
```

The active-owner and terminated-owner cases are covered by the cross-platform
tests:

```bash
cargo test --manifest-path ../../../Cargo.toml --lib storage::lock::tests
```
