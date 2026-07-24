# external-commands (single feature: external subcommand binaries)

This board demonstrates pinto's Git-style **external subcommand** contract. When
you run a command that is not built in, pinto looks for an executable named
`pinto-<command>` beside its own binary or on `PATH`, runs it, and forwards your
arguments plus a small contract environment. Built-in commands are always
resolved by pinto itself and are never shadowed by a same-name executable on
`PATH`.

The demo ships a runnable mock plugin, [`plugins/pinto-standup`](plugins/pinto-standup),
so the feature can be exercised without installing anything. See
[`docs/plugin-contract.md`](../../../docs/plugin-contract.md) for the complete
contract.

## Try it

Put the demo's `plugins/` directory on `PATH`, then drive pinto through the
repository binary from this directory:

```bash
export PATH="$PWD/plugins:$PATH"

# Unknown command → pinto runs `pinto-standup` and forwards the arguments.
cargo run --manifest-path ../../../Cargo.toml -- standup

# `--dir PATH` is delivered to the plugin as PINTO_DIR.
cargo run --manifest-path ../../../Cargo.toml -- --dir "$PWD" standup today

# The root help screen lists installed plugins with a concise summary and does
# not require an initialized board.
cargo run --manifest-path ../../../Cargo.toml -- help
```

`pinto standup` prints the host version and contract version it received, the
forwarded arguments, and a status breakdown of this board, for example:

```text
pinto-standup  (host pinto 0.3.3, contract v1)
open work by status:
  todo         1
  in-progress  0
  review       0
  done         0
```

## How the mock plugin works

`pinto-standup` is a small POSIX shell script — an external command is just an
executable, in any language. It demonstrates each part of the contract:

- **Discovery**: it is named `pinto-standup`, so `pinto standup` finds it.
- **Help summary**: `pinto-standup --help` prints a one-line summary, which is
  what `pinto help` shows next to the command.
- **Argument forwarding**: everything after `standup` arrives as ordinary argv.
- **Contract environment**: it reads `PINTO_DIR` (from `--dir`), plus
  `PINTO_HOST_VERSION` and `PINTO_PLUGIN_CONTRACT_VERSION`.
- **File-format boundary**: it reads the plain-text `.pinto/tasks/*.md` board
  directly instead of any private pinto internals, because the documented file
  format is the extension compatibility boundary.

To adopt your own command, drop an executable named `pinto-<name>` anywhere on
`PATH` (or beside the `pinto` binary) and run `pinto <name>`.
