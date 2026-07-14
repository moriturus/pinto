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

## SQLite journal mode

SQLite uses its default journal mode. WAL would improve concurrent reader and
writer behavior, but it introduces `-wal` and `-shm` companion files and is
not useful for the short-lived, write-serialized CLI workload. Revisit this
decision only if a long-running local process is introduced.

## SQLite schema metadata

The current SQLite schema version is `1`. Every newly created database contains an extensible
`metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL)` table with these reserved
entries:

- `schema_version = "1"` identifies the normalized table layout.
- `format = "pinto-sqlite"` identifies the pinto SQLite storage format.

Opening a new database stamps these entries. Existing databases with missing,
unknown, or malformed schema metadata fail with an actionable compatibility
error. Pinto does not migrate, convert, downgrade, or promise that an older
database layout can be read by this first release. A future schema change must
increment the version, document the new metadata, add an explicit migration
plan, and update the compatibility check and tests together.

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
because their source wording contains the most actionable repair detail.
