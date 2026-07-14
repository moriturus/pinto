# markdown-rendering (single feature: Markdown in `show` and Kanban details)

This demo shows PBI bodies rendered as **Markdown** — styled headings, bullets,
task checkboxes, inline/fenced code, tables, and blockquotes — in both `pinto
show` and the Kanban details popup. Rendering is the default; a per-invocation
`--plain` flag (and the `[display] markdown = false` config) opts out into raw
text. Malformed Markdown falls back to plain text instead of failing.

The four PBIs cover the range:

- the first seeded PBI — a rich body: headings, emphasis, lists, checkboxes, a fenced code
  block, and a blockquote.
- the second (`in-progress`) — a Markdown **table** plus a link.
- the third — deliberately **malformed** Markdown (unterminated fence, broken table,
  dangling quote) to show the safe plain-text fallback.
- the fourth — no body: the details popup shows its usual placeholder.

Run the commands from this directory:

```bash
# Rendered (default): headings lose their `#`, bullets/tables/code are styled.
cargo run --manifest-path ../../../Cargo.toml -- show T-1

# Opt out for one invocation: the raw Markdown is printed verbatim.
cargo run --manifest-path ../../../Cargo.toml -- show T-1 --plain

# The Kanban popup (press `v` on a card) uses the same rendering path.
cargo run --manifest-path ../../../Cargo.toml -- kanban
```

To opt out everywhere, set `[display] markdown = false` in `.pinto/config.toml`;
both `show` and `kanban` then display raw text.
