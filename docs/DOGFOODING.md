# Dogfooding workflow

pinto hosts its own backlog in `.pinto/` at the repository root (see
[`migration.md`](migration.md)). Always run pinto itself when validating a change or updating
the backlog. Do not replace this verification with direct file editing.

## Principles

1. **Run through `cargo run`** — invoke the current worktree with `cargo run -- <command> ...`
   instead of relying on an installed or pre-built binary.

   ```bash
   cargo run -- list --label review
   cargo run -- show <ID-from-list>
   ```

   Replace `<ID-from-list>` with an ID returned by the preceding list command.

   Use `cargo run --quiet --` when repeated commands would make build output noisy.

   Automation plans can be supplied inline, from a file, or from standard input. Use `--dry-run`
   before applying a plan with several writes.

   ```bash
   cargo run -- automate --plan plan.json --dry-run --json
   cargo run -- automate --plan - --json < plan.json
   ```

2. **Do not edit `.pinto/tasks/*.md` directly** — add, transition, rank, and remove items through
   the `add`, `move`, `edit`, and `rm` commands. Direct editing is reserved for data recovery that
   cannot be expressed through a command.

   Ordinary read commands do not acquire the board-wide write lock and therefore remain
   non-blocking, but their multi-resource results are not snapshot-isolated. Use
   `cargo run -- export --json` when a shell or agent needs PBIs, Sprints, configuration, and the
   shared DoD from one consistent board state.

3. **Use the `default` template** — apply `.pinto/templates/item/default.md` with
   `--template default` for items whose body follows the Summary and Acceptance Criteria format.
   This keeps item bodies consistent with the Definition of Done.

   ```bash
   cargo run -- add "Title" --template default --edit
   ```

## Using `--edit` without a terminal

`add --edit`, `edit` without field options, and the `e` key in `kanban` launch `$VISUAL` or
`$EDITOR` to edit the body (see [`src/cli/editor.rs`](../src/cli/editor.rs)). In CI or an agent
without an interactive TTY, point `$EDITOR` at a shim that only writes the supplied file:

```bash
cat > /tmp/editor_shim.sh <<'EOS'
#!/bin/sh
# $1 is the temporary file prepared by pinto; replace it with $CONTENT.
printf '%s\n' "$CONTENT" > "$1"
EOS
chmod +x /tmp/editor_shim.sh

CONTENT="$(cat <<'BODY'
# **Summary**

...

# **Acceptance Criteria**

- [ ] ...
BODY
)" EDITOR=/tmp/editor_shim.sh cargo run -- add "Title" --template default --edit
```

The editor command is split on spaces, and the temporary file path is appended as the final
argument. A multi-word command such as `code --wait` is supported. The shim only needs to write
the first argument.

For multiple items, wrap the same invocation in a shell function or loop.

## Verification

1. Run the target operation with `cargo run -- <command>`.
2. Inspect the result with `cargo run -- show <ID>`, `list`, or `board`.
3. Remove an accidentally created item with `cargo run -- rm <ID> --force` before retrying.
4. Finish with `mise run release-check` (check, coverage, dependency audit, and deny policy).

`mise run coverage` measures all source paths with all features enabled; this repository has no
source-coverage exclusions. TUI lifecycle and storage-boundary regressions therefore remain part
of the same 95% line-coverage gate.

This is also the standard procedure for satisfying the dogfooding requirement in the Definition of
Done.
