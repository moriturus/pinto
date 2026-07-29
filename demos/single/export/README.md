# export (single feature: complete board JSON)

This demo board contains two PBIs assigned to one Sprint, a Sprint Retro, a
Sprint Review, and a shared Definition of Done. Export the complete active board
from this directory with:

```bash
cargo run --manifest-path ../../../Cargo.toml -- export --json
```

The JSON object contains `items`, `sprints`, `retros`, `reviews`, `config`, and
`dod`. The item, Sprint, Retro, and Review fields match the existing `--json`
commands, including explicit `sprint_id` parent references and UTC RFC 3339
timestamps. The command is read-only and waits for writers while assembling one
consistent snapshot.

Ordinary `list`, `show`, and `board` reads remain non-blocking and do not
provide board-wide snapshot isolation. Use this export command from shell
scripts or agents whenever records from different resource collections must
come from the same board state.
