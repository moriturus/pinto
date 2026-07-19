# Stability decisions

## Item ID allocation

All backends reserve issued PBI IDs in the plain-text `.pinto/issued_ids`
history. Physically deleted IDs remain reserved, and the history is shared when
switching between storage backends. The file backend also scans current task and
archive filenames so boards created before the history was introduced do not
immediately reuse an existing ID.
Normal pinto writes are serialized by the OS-level advisory lock on
`.pinto/.lock`; do not directly create or rename task files while a pinto
write is running. The lock is released automatically when its owner process
terminates, so a crash does not require PID-based stale detection. Manual
recovery or bulk editing must be performed with no other pinto process active,
followed by `pinto list` to validate the board. We use a small cross-platform
file-lock dependency rather than adding a database or a heavier coordination
service to this local-first tool.

The file backend validates the complete task and archive file set before each
operation. A malformed or inconsistent record therefore stops the operation,
including an operation that targets an otherwise healthy item. This fail-fast
behavior preserves data integrity; repair the reported file and rerun `pinto
list` before continuing.

Writes wait up to five seconds for another process by default. The lock is held
through the complete Git-backed operation, including its commit, so the commit
cannot be interleaved with another write. On a slow filesystem or with a slow
Git hook, set `PINTO_LOCK_TIMEOUT_SECS` to a larger non-negative integer. A
timeout is recoverable: retry the command after the other writer finishes.

## Atomic replacement and durability

File-backed records are written to a temporary file in the same directory and
then atomically renamed into place. A process crash therefore does not leave a
half-written record. Pinto does not call `fsync` on the file or its parent
directory, so a power loss can still lose a recent write; use the Git backend
when the repository's normal commit history is also needed for recovery.

## Git commit boundaries and recovery

The Git backend commits one complete pinto service operation at a time. It uses
a temporary index, so staged, unstaged, and untracked changes that existed
before the operation are not mixed into the pinto commit; the caller's staged
index is restored after a successful commit. The transient `.pinto/.lock` is
excluded from every commit, including cleanup of that path from older
checkouts that tracked it.

If the Git commit fails, pinto leaves the durable Markdown or configuration
change in the worktree and does not replace the original index. Check `git
status`, fix the reported Git problem, then retry or commit the durable files
manually. A rare failure while refreshing the real index is reported after
`HEAD` has been updated; use `git status` as the recovery source of truth. This
is an intentional recoverable failure contract: a failed commit does not roll
back or discard board data.

## Configuration and data compatibility

`.pinto/config.toml` is shared board configuration. It uses a strict schema, so
an older binary may reject a board configuration containing a key introduced by
a newer release. Every release that adds a board key must state whether older
binaries can read it. Before downgrading, restore the configuration from Git or
a backup, or remove the documented newer keys; do not copy personal settings
into the board file. If the older binary still rejects the file, use the newer
binary to move the board back to the documented compatible representation.

PBI and Sprint Markdown with TOML frontmatter are file-backed board data and
must remain under `.pinto/`; their compatibility is separate from the board
configuration schema. SQLite has an independent versioned schema and its own
migration and downgrade procedure, documented below. JSON is a machine-readable
CLI output contract, not a persistence backend or a configuration source; an
export does not contain personal keybindings.

## SQLite journal mode

SQLite uses its default journal mode. WAL would improve concurrent reader and
writer behavior, but it introduces `-wal` and `-shm` companion files and is
not useful for the short-lived, write-serialized CLI workload. Revisit this
decision only if a long-running local process is introduced.

## SQLite support policy

SQLite remains a supported optional backend for teams that need normalized
local storage, but it is not enabled by default. The file backend remains the
plain-text and Git-diff compatibility boundary; SQLite is an explicit exception
with a separate schema and migration contract. SQLite support is not a removal
target merely because the default workflow is file-backed.

Future SQLite schema work must increment the schema version, document the
affected users and recovery path, and provide an explicit migration plan before
the schema changes. The release compatibility check and an integration test
must be updated in the same change.

## SQLite schema v1 to v2 compatibility

The current SQLite schema version is `2`. Every newly created database contains an extensible
`metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL)` table with these reserved
entries:

- `schema_version = "2"` identifies the normalized table layout, including close-time Sprint
  spillover columns.
- `format = "pinto-sqlite"` identifies the pinto SQLite storage format.

### Affected users

This breaking change affects users who have a SQLite board created by a version-1-compatible pinto
binary. Version 2 added `closed_at`, `spillover_points`, `spillover_items`, and
`unestimated_spillover_items` to the `sprints` table so that closing a Sprint preserves a snapshot
of unfinished work. A version-1 database does not contain those columns and is not silently altered.

### Symptoms

Opening a version-1 database with a version-2 binary fails before any board operation with an
unsupported-schema error similar to:

```text
unsupported SQLite schema ... found version "1", but pinto supports version 2
```

The database is left untouched. The same fail-fast behavior applies when schema metadata is
missing, unknown, or malformed. There is no automatic SQLite migration or downgrade.

### Back up before upgrading

Stop all pinto processes and make a byte-for-byte backup of `.pinto/board.sqlite3` before changing
the binary or attempting recovery. Keep the original database read-only and perform recovery on a
copy. If the board also has a file-backend source, back up that directory or commit it with Git as
well. Do not fix the schema by adding columns manually: that can lose the domain-level guarantees
and does not convert the stored data contract.

### Downgrade and recovery

To preserve a version-1 SQLite board, use a version-1-compatible pinto binary on the backup and
run `pinto migrate --to file`. Then upgrade pinto and, after checking the file-backed board, run
`pinto migrate --to sqlite` to create a new version-2 database. This file-backend round trip is the
supported conversion path; version 2 cannot downgrade a database in place.

If the old binary is unavailable, restore the SQLite backup when a compatible binary can be
obtained, or recreate the SQLite board from its file-backed source and verify it with `pinto list`
and `pinto doctor`. Never overwrite the only backup while testing recovery.

Opening a new database stamps the version-2 metadata. A future schema change must increment the
version, document the new metadata, add an explicit migration plan, and update the compatibility
check and tests together.

## Permanent removal

`pinto rm` archives by default. `pinto rm --force` is intentionally the
explicit irreversible escape hatch for recovery and automation; it does not
add an interactive prompt, which would break scripted use. Use the default
archive operation whenever recovery may be needed. Permanent removal is
rejected while an active PBI refers to the target through `parent` or
`depends_on`; remove those links first.

## Input size limits

PBI titles, bodies, labels, and other text fields have no application-level
size limit. The expected workload is a local, human-maintained text board, and
adding arbitrary limits would reject legitimate planning notes while providing
little practical protection. Filesystem errors remain actionable and are
reported to the user. Revisit this choice only if pinto gains an untrusted or
network-facing input boundary.

## Error-message localization

Command help and pinto-owned user-facing messages use the existing Fluent
localization catalogs. Structured domain errors select a catalog entry from a
stable variant code, so a localized CLI does not show only a translated
`error:` prefix. Operating-system, Git, TOML, and other dependency diagnostics
are an explicit exception: they are inserted verbatim into a localized wrapper
because their source wording contains the most actionable repair detail. The
English catalog is kept identical to `Error::to_string()` for every public
variant; `Display` is the library fallback and `localized()` is used only at the
CLI/TUI rendering boundary.
