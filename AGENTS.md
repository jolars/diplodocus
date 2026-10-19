# Repository Guidelines

## Project Structure & Module Organization

Diplodocus is a Rust 2024 CLI that builds documentation sites for Python, R,
Julia, and Rust packages.

- `src/` contains the CLI, extractors, documentation IR, assembly, execution,
  snapshots, and rendering modules.
- `src/rendering/assets/` holds embedded site assets; `assets/brand/` contains
  branding.
- `tests/` contains Rust integration tests, `fixtures/`, golden files in
  `snapshots/`, and Playwright tests in `browser/`.
- `docs/` contains guides, architecture decisions, and contracts.
  `examples/monorepo/` exercises all four languages; `examples/vitepress/`
  demonstrates an external frontend.

## Build, Test, and Development Commands

Run commands from the repository root. Enter `devenv shell` for the pinned Rust
toolchain, language runtimes, Jupyter kernels, Node, and Chromium. Install
browser tools with `npm ci --ignore-scripts`.

- `cargo build --locked`: compile the library and CLI.
- `cargo test --locked`: run Rust unit, integration, and documentation tests.
- `cargo run --locked -- check`: validate the project's documentation sources.
- `cargo run --locked -- build --output site/docs`: build the project site.
- `task preview`: watch generator changes and serve the monorepo example with
  live reload.
- `cargo fmt --all -- --check`: check Rust formatting.
- `cargo clippy --locked --all-targets --all-features -- -D warnings`: run CI's
  lint checks.

## Coding Style & Naming Conventions

Use rustfmt defaults, four-space Rust indentation, `snake_case` functions and
modules, and `PascalCase` types. Match existing two-space JavaScript and
TypeScript indentation. Document public Rust APIs, preserve the unsafe-code
prohibition, and explain nonobvious decisions in comments. Write Markdown using
GFM, as configured in `panache.toml`.

## Testing Guidelines

Use descriptive `snake_case` Rust test names and shared helpers in
`tests/support.rs`. Add focused regression tests for behavior changes; review
Snapbox golden-file changes in `tests/snapshots/`. No numeric coverage threshold
is enforced.

Run `cargo test --locked --test rust_extraction` to target one integration
suite. For presentation changes, run `npm run site-test` and inspect
`npm run site-capture` screenshots. Use `npm run vitepress-test` for external
frontend integration changes. See `docs/development/browser.md` for details.

## Commit & Pull Request Guidelines

Follow the history's Conventional Commits, such as
`fix: validate Python checkouts` or `refactor(ui): fix sidebar spacing`. Keep
titles short and ASCII. Reference applicable issues with `Fixes #123`,
`Closes #123`, or `Refs #123`.

PRs should describe the resulting behavior, link relevant issues, report
validation, and include screenshots for visual changes. Keep generated sites and
browser artifacts out of commits.
