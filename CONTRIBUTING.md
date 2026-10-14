# Contributing

Clear Compare is an MIT-licensed Rust workspace. Small, tested changes are welcome. Check the [feature matrix](docs/feature-matrix.md) before starting a new module.

## Set up

Install Rust through rustup and the system dependencies documented in the README. Then:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
cargo install tauri-cli --version '^2' --locked
```

## Verify

```sh
cargo fmt --all --check
cargo test -p clear-contracts -p clear-core -p clear-cli --locked
cargo clippy -p clear-contracts -p clear-core -p clear-cli --locked -- -D warnings
trunk build --release
cargo check -p clear-desktop --locked
```

Use the release build to check actual desktop behavior. Browser-only mocks cannot prove file access, dialogs or synchronization.

## Pull requests

Explain the observable behavior, affected platforms, validation and remaining limitations. For synchronization changes, include tests for exclusions, destination changes, backup failures, symbolic links and interrupted writes. Do not use private files as fixtures.

Do not introduce network services, telemetry, shell execution or unbounded background tasks without an explicit design decision. Dependencies must work on the declared platforms and have compatible licenses.
