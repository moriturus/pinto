# i18n demo

This board demonstrates pinto-owned CLI messages in English and Japanese. It
contains two PBIs, a dependency, a linked commit, and a shared Definition of
Done so the success paths can be compared without changing the stored board
data.

Run the commands from this directory:

```bash
LC_ALL=en_US.UTF-8 cargo run --manifest-path ../../../Cargo.toml -- dep add T-1 T-2
LC_ALL=ja_JP.UTF-8 cargo run --manifest-path ../../../Cargo.toml -- dep add T-1 T-2
LC_ALL=ja_JP.UTF-8 cargo run --manifest-path ../../../Cargo.toml -- dod
LC_ALL=ja_JP.UTF-8 cargo run --manifest-path ../../../Cargo.toml -- migrate --to file
LC_ALL=en_US.UTF-8 cargo run --manifest-path ../../../Cargo.toml -- show T-404
LC_ALL=ja_JP.UTF-8 cargo run --manifest-path ../../../Cargo.toml -- show T-404
LC_ALL=ja_JP.UTF-8 cargo run --manifest-path ../../../Cargo.toml -- add ""
```

The `show T-404` commands demonstrate that the English Fluent error is the same
as the library `Error::to_string()` fallback while Japanese uses the selected
catalog. The last command demonstrates actionable localized validation. OS, Git,
and TOML diagnostics remain in their original wording inside the localized error
wrapper, so repair details are not lost. `list --json` and `board --json` remain
machine-readable in either locale.
