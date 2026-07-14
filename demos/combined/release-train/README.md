# release-train (combined: cross-sprint report)

This release-level demo contains three closed Sprints (`S-1`/`S-2`/`S-3`) and one active Sprint,
`S-4`. It combines velocity, burndown, and cycle-time reports, including unestimated completed
work and a carry-over item. The fixed dataset is dated 2026-05 through 2026-06.

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint velocity            # completed points by Sprint
cargo run --manifest-path ../../../Cargo.toml -- sprint velocity --recent 3 # limit to the three latest Sprints
cargo run --manifest-path ../../../Cargo.toml -- sprint burndown S-4        # burndown for the active Sprint
cargo run --manifest-path ../../../Cargo.toml -- cycletime                  # cycle/lead time for the release
cargo run --manifest-path ../../../Cargo.toml -- cycletime -S S-3           # limit to one Sprint
```
