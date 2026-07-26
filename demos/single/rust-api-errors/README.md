# Rust API error-contract demo

This demo accompanies P-54. It provides a small board for reviewing the public
service and persistence API documentation, including validation errors,
multi-record recovery, and retry guidance.

Run the read-only board checks from this directory:

```bash
cargo run --manifest-path ../../../Cargo.toml -- list --long
cargo run --manifest-path ../../../Cargo.toml -- board
cargo run --manifest-path ../../../Cargo.toml -- show T-1
```

Run the documentation checks from the repository root:

```bash
cargo test --test docs selected_public_result_apis_have_error_contract_guards --locked
cargo test --doc --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
cargo clippy --all-targets --all-features -- -D warnings
```

The demo board is intentionally plain text and should be changed with pinto
commands. The Rustdoc comments are the source of truth for whether a failed
operation can leave durable partial changes and whether retrying is safe.
