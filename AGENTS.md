# Working on Diplodocus

Use `devenv shell` for the pinned Rust, Python, R, Node, and browser environment.
Run `npm ci --ignore-scripts` after checkout or changes to `package-lock.json`.
Devenv supplies Chromium; no `playwright install` step is needed.

## Where to work

- `src/extractors/`: static Python and R extraction.
- `src/documents.rs` and `src/documents/`: authored Markdown and QMD parsing.
- `src/assembly/`, `src/validation/`, and `src/snapshots/`: workspace assembly,
  reference resolution, and the portable SQLite snapshot.
- `src/site.rs` and `src/rendering/`: site routes, HTML, CSS, and search.
- `src/commands/`: command pipeline and watched preview server.
- `examples/monorepo/`: small static R/Python site for browser development.
- `tests/fixtures/acceptance/`: broader extraction and execution corpus.

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

- `site-dev` builds the current Rust binary and serves the monorepo example at
  `http://127.0.0.1:8000/`. Stop it with Ctrl-C.
- Restart `site-dev` after changing Rust. The preview watcher observes
  documentation inputs, not the renderer's Rust source. Refresh the browser
  after watched documentation rebuilds.
- `site-test` runs Playwright against isolated sources and an automatically
  assigned port. `site-capture` also saves screenshots of representative pages.
- Both commands print the run's artifact directory under `artifacts/browser/`.
- `playwright-cli` is the repository wrapper for interactive inspection. Use a
  named session and close that session when finished. Its official skill is in
  `.agents/skills/playwright-cli/`.

See [the browser workflow](docs/development/browser.md) for command arguments,
port and output overrides, and dependency updates.
