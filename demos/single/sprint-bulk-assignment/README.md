# Sprint bulk assignment

This demo shows `sprint add` selecting PBIs by workflow status in backlog rank order.

The checked-in board contains:

- `S-1`: the first two `todo` PBIs assigned with `--limit 2`;
- `S-2`: every `review` PBI assigned without a limit;
- the remaining `todo` and `in-progress` PBIs are both unassigned.

Run these commands from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint add S-1 --status todo --limit 2
cargo run --manifest-path ../../../Cargo.toml -- sprint add S-2 --status review
cargo run --manifest-path ../../../Cargo.toml -- list --sprint S-1 --json
```

The first command is idempotent for PBIs already assigned to `S-1`. Use `--limit` to cap new
assignments; omitting it assigns all matching PBIs. A PBI already assigned to another Sprint is
reported as an error and the bulk operation makes no assignments.
