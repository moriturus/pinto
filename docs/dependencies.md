# Dependency decisions

## `rusqlite` (optional SQLite backend)

`rusqlite` implements the optional, non-default SQLite backend. The default
build keeps the file backend and does not compile this dependency; users opt in
with `--features sqlite` when a normalized local database is useful for their
Scrum board.

The dependency uses `bundled SQLite` so the opt-in build does not require a
system SQLite installation. That increases compile time and package size for
the opt-in path, but keeps the supported feature reproducible across hosts. The
file backend remains the lightweight default and the SQLite support is not a
removal target.

SQLite has an independent versioned schema. Any schema change must record its
dependency and persistence impact, provide an explicit migration or recovery
plan, and update the compatibility tests and guidance together.

## `serde_json`

`serde_json` remains an unconditional dependency. It powers both Automation
Plan parsing and the public `--json` output supported by multiple CLI
commands. Feature-gating it would either remove an advertised default CLI
capability or add configuration complexity without materially reducing the
default dependency graph.

## Selective Clippy pedantic lints

CI enables `must_use_candidate` and `redundant_closure_for_method_calls` in
addition to the default Clippy lint set. Both identify actionable mistakes or
mechanical simplifications and can be kept warning-free without obscuring the
implementation.

The full `clippy::pedantic` group is intentionally deferred because it mixes
these useful checks with high-volume, preference-driven warnings that would
require broad churn. In particular, `missing_errors_doc` currently reports on
most service functions; adopting it should be a focused documentation change
that explains each error contract rather than adding generic boilerplate.


## `termimad`

`termimad` renders PBI Markdown bodies for `pinto show` and the Kanban details
popup. It is the established, focused crate for terminal Markdown and
reuses the `crossterm` backend `ratatui` already pulls in, so it adds no new
terminal stack. Rolling our own Markdown renderer (headings, lists, tables,
code) would be far more code and more error-prone; a raw ANSI passthrough would
not strip syntax or wrap to width.

The TUI popup does not take a second dependency to bridge `termimad` into
`ratatui`: the shared renderer emits ANSI once and a small in-tree SGR parser
(`src/cli/markdown.rs`) converts each line into ratatui spans, so both display
paths share a single rendering. Redirected `show` output uses `termimad`'s
colourless skin so pipes and files stay clean text.

## `tempfile`

`tempfile` is an unconditional dependency because editor-backed commands need
an owner-private temporary buffer with exclusive creation, collision retries,
Unix 0600 permissions, and cleanup on success, failure, or panic. Its
`NamedTempFile` primitive provides those guarantees for the blocking
`$EDITOR` boundary. `tokio::fs` provides asynchronous file operations but not
this complete temporary-file lifecycle, so rebuilding it locally would add
security-sensitive code without helping the synchronous editor process.
