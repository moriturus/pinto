# kanban-card-meta (single feature: points and assignee on Kanban cards)

This demo shows story points (`◆`) and the assignee (`@`) directly on each Kanban
card, so workload and ownership are visible without opening the details popup.

The four `todo` PBIs cover every combination:

- the first — points **and** assignee (`◆ 5  @alice`);
- the second — points only (`◆ 8`);
- the third — assignee only (`@bob`);
- the fourth — neither: the card keeps its previous single-line layout (no meta line).

Another seeded PBI sits in `in-progress` with both set (`◆ 3  @carol`).

Run the commands from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- board
cargo run --manifest-path ../../../Cargo.toml -- kanban
```

In `kanban`, the meta line is drawn in a muted color under the title and aligns
with the title column, mirroring the dependency line. Set points with
`edit <ID> --points N` and the assignee with `edit <ID> --assignee NAME`; clear
either with `--points ""` / `--assignee ""` and the meta line updates on reload.
