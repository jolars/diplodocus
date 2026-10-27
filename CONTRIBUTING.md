# Contributing

Diplodocus is a Rust CLI that builds documentation sites for Python, R, Julia,
and Rust packages. The [configuration guide](docs/guide/configuration.md) and
[authoring guide](docs/guide/authoring.md) describe its current behavior. See
the [changelog](CHANGELOG.md) for released changes.

## Development environment

Run commands from the repository root. Enter `devenv shell` for the pinned Rust
toolchain, language runtimes, Jupyter kernels, Node, and Chromium.

```sh
cargo build --locked
cargo run --locked -- check
cargo run --locked -- build --output site/docs
```

Use `task preview` to watch generator changes and serve the four-language
monorepo example with live reload.

## Checks

Add focused regression tests for behavior changes, and review any changes to
golden files in `tests/snapshots/`. Run the relevant integration suite while
developing, then run the Rust checks:

```sh
cargo test --locked
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
```

For presentation changes, install browser tools with `npm ci --ignore-scripts`,
run `npm run site-test`, and inspect screenshots from `npm run site-capture`.
Run `npm run vitepress-test` for changes to the external frontend integration.
The repository's [browser development
guide](https://github.com/jolars/diplodocus/blob/main/docs/development/browser.md)
describes these workflows.

## Pull requests

Follow the existing Rust and JavaScript styles, document public Rust APIs, and
update the guides when behavior changes. Use short Conventional Commit titles.
Describe the resulting behavior in pull requests, link relevant issues, report
validation, and include screenshots for visual changes. Keep generated sites and
browser artifacts out of commits.
