# External command contract

Pinto has two command classes:

- Built-in commands are provided by the single `pinto` binary and are resolved
  internally; they are never delegated to an external executable.
- External commands extend Pinto through executables named `pinto-<segment>` or
  `pinto-<segment>-<segment>...` discovered beside the running binary or on
  `PATH`.

External commands run with the invoking user's permissions. This mechanism is
not a sandbox, privilege boundary, or trust boundary. A `PATH` entry that a
user can change already permits that user to run a program; Pinto does not add
privilege through delegation.

## Discovery and resolution

Each command segment must be non-empty and contain only ASCII letters, digits,
`-`, `_`, or `.`. A command such as `pinto team report` first looks for the
longest available name, `pinto-team-report`, then falls back to
`pinto-team`.

Pinto searches the directory beside the running executable first. It then
searches non-empty entries in `PATH` in their declared order. The current
directory is never inserted implicitly; an explicit `PATH` entry such as `.`
remains the caller's responsibility. Consequently, an executable beside the
running Pinto binary takes precedence over a stale same-name executable found
later on `PATH`. A built-in command name (or its short alias) is always resolved
by Pinto itself and is never shadowed by a `pinto-<name>` executable on `PATH`.

The root help screen discovers external commands without opening a board. It
invokes each candidate with `--help`, uses a short first-line summary when the
candidate responds promptly, and omits broken or slow candidates. Missing
commands produce a user-facing error and never panic.

## Process contract

Pinto starts the resolved executable directly and forwards each argument as a
separate argv element; it does not interpolate a shell command string. A
supplied `--dir PATH` is forwarded through `PINTO_DIR=PATH`. The value may be a
project directory or its `.pinto` directory. No `PINTO_DIR` is added when the
flag was omitted; an existing caller-provided `PINTO_DIR` remains inherited.

Every delegated process receives:

- `PINTO_HOST_VERSION`: the Pinto package version, such as `0.3.3`.
- `PINTO_PLUGIN_CONTRACT_VERSION`: the external command contract version,
  currently `1`.

External commands do not require an initialized board. A child process's exit
code is returned to the caller; a process terminated without a numeric exit
code is reported as an internal failure.

The contract version increments for incompatible changes to naming, discovery,
argument, environment, or exit-status behavior. Backward-compatible additions
remain within the current contract version.

The file backend remains the compatibility boundary for extensions. Its board
data stays in the plain-text `.pinto` layout, and external commands must use
documented CLI behavior or the file format rather than private Pinto CLI
modules.

## Automation callers

Pinto validates command-name segments, but it does not validate arbitrary plugin
arguments or sandbox the child process. An automation caller that accepts
untrusted input must allowlist command names, validate argument values, and use
its own end-of-options marker (`--`) where its argument parser supports one.
Treat the selected external command and the board it can modify as trusted
inputs with the caller's user permissions.
