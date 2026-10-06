# Working on Diplodocus

Use `devenv shell` for the pinned Rust, Python, R, Node, and browser environment.
Run `npm ci --ignore-scripts` after checkout or changes to `package-lock.json`.
Devenv supplies Chromium; no `playwright install` step is needed.

The [contributor handoff](docs/development/handoff.md) maps the build pipeline,
source modules, and focused test targets. The small example is in
`examples/monorepo/`; the broader extraction and execution corpus is in
`tests/fixtures/acceptance/`.

## Verification

For Rust changes, run `cargo fmt --all -- --check`, the relevant integration
tests, and `cargo clippy --locked --all-targets --all-features -- -D warnings`.
The full suite is `cargo test --locked --all-targets`; execution tests need the
devenv kernels.

For rendering, styling, navigation, or search changes, run `site-test` and use
`site-capture` to inspect the affected desktop and mobile screenshots. Capturing
also runs the browser tests, so one `site-capture` run can satisfy both checks.
Use the generated report and failure traces to diagnose failures. Do not update
goldens solely to make a failure pass.

Tests that write snapshots, caches, or sites must use temporary copies of source
fixtures. Keep generated artifacts out of version control.

## Browser workflow

See [the browser workflow](docs/development/browser.md) for preview commands,
test artifacts, interactive inspection, and dependency updates. Restart
`site-dev` after changing Rust; its watcher observes documentation inputs.
