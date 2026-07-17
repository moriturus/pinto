# Installation

## Using a released package

Install the latest published 0.2.0 binary with Cargo:

```bash
cargo install pinto-cli --version 0.2.0
pinto --version
```

The crates.io package is named `pinto-cli`; it installs the `pinto` binary.

Rust 1.89 or newer is required to build the current project. If a release is
not yet available on crates.io, install from a checkout instead.

## Installing from source

Clone the repository and install the binary from its workspace:

```bash
git clone https://github.com/moriturus/pinto
cd pinto
cargo install --path . --locked
pinto --version
```

The source install uses the committed `Cargo.lock`; `--locked` makes dependency
resolution fail instead of silently changing that lockfile.

## Contributor setup

Contributors use [mise](https://mise.jdx.dev) to install the Rust toolchain,
mdBook, and the project quality tools:

```bash
mise install
mise run check
```

The `check` task runs the Rust tests, lint, Rust API documentation, the mdBook
build, and formatting checks. To preview this book while editing it, run:

```bash
mdbook serve
```

The generated site is written to `target/book/`, which is a build artifact and
is not part of the source tree.
