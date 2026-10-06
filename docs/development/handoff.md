# Contributor handoff

Start with the [small R/Python example](../../examples/monorepo/README.md) to
see a complete build. The [project guide](../guide/index.md) describes the
user-facing configuration and commands. This page is a map for changing the
implementation.

## How a build moves through the code

`src/main.rs` parses the CLI; `src/commands/` coordinates the operations. The
`build` command follows this path:

1. `src/configuration.rs` and `src/paths.rs` load declarations and resolve
   source boundaries.
2. `src/extractors/` reads Python and R packages without importing or running
   them. `src/documents/` parses Markdown and QMD. `src/assembly/` combines
   those inputs into the types in `src/ir/`.
3. `src/validation/` resolves references and assets. On Linux,
   `src/execution/` runs authorized QMD cells through Jupyter and validates
   their outputs; `src/assembly/execution.rs` joins the results to the
   workspace. The [execution map](../design/execution.md) describes this path.
4. `src/snapshots/` validates and publishes a portable SQLite database.
   `generate` can load this database without the original sources or kernels.
5. `src/site.rs` builds routes and navigation from the snapshot;
   `src/rendering/` writes the HTML, assets, CSS, and search data. `serve` in
   `src/commands/preview.rs` watches inputs and rebuilds the site.

The [snapshot schema](../design/snapshot-schema.md) is the contract between
extraction and generation. The [browser workflow](browser.md) covers local
preview and visual inspection.

## Where to make and verify a change

Run commands from the repository root inside `devenv shell`. After checkout or
a change to `package-lock.json`, run `npm ci --ignore-scripts` before browser
checks. The `--test` names below are filenames in `tests/`, so they can be
combined in one `cargo test` invocation.

| Change | Main code | Focused check |
|:-------|:----------|:--------------|
| Configuration and input paths | `src/configuration.rs`, `src/configuration_validation.rs`, `src/paths.rs` | `cargo test --locked --test configuration --test path_resolution` |
| Static Python or R extraction | `src/extractors/` | `cargo test --locked --test python_surface --test r_extraction --test static_extractor_contract` |
| Authored Markdown or QMD | `src/documents/` | `cargo test --locked --test documents --test qmd_preparation` |
| Assembly and references | `src/assembly/`, `src/validation/` | `cargo test --locked --test workspace_assembly --test snapshot_validation` |
| Execution and caching | `src/execution/`, `src/assembly/execution.rs` | `cargo test --locked --test execution_contract --test execution_cache --test jupyter_real_kernels` |
| SQLite snapshot | `src/snapshots/` | `cargo test --locked --test snapshot_schema --test snapshots_storage --test snapshot_publication` |
| Site or command pipeline | `src/site.rs`, `src/rendering/`, `src/commands/` | `cargo test --locked --test site_generation --test extract_command` |

The execution tests use Python and R kernels from devenv. For rendering,
navigation, styling, or search changes, run `site-capture` and inspect its
desktop and mobile screenshots; it also runs the Playwright tests. Use
`site-test` when screenshots are not needed. Both commands create isolated
source copies and print their artifact directory.

Before handing over a Rust change, run `cargo fmt --all -- --check` and
`cargo clippy --locked --all-targets --all-features -- -D warnings`. Run
`cargo test --locked --all-targets` when a change spans several stages or
before a release. [CI](../../.github/workflows/ci.yml) also checks rustdoc,
snapshot encoding with insertion-ordered JSON maps, and the browser capture.
Tests that publish sites, caches, or snapshots must use temporary copies of
source fixtures; generated artifacts stay out of version control.

When an intentional behavior change alters golden snapshots, rerun the focused
test with `SNAPSHOTS=overwrite`, inspect `git diff -- tests/snapshots`, then run
the test again without overwrite. Do not accept snapshots merely to make a
failure pass.
