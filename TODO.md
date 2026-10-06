# Project status and remaining work

Reviewed October 6, 2026. This page tracks work that remains before the first
release and the collaborator handoff. It replaces the old milestone-by-milestone
implementation log; Git history preserves that log and its completed checklists.
A checked implementation task is not, by itself, a release gate.

## What works today

| Area | Current implementation | Evidence to start with |
| --- | --- | --- |
| Workspace input | Explicit repository, package, content, concept, relationship, and execution configuration; static Python and R extraction; GFM and QMD parsing | `src/configuration.rs`, `src/extractors/`, `src/documents/`; `tests/acceptance_gate.rs` |
| Authored execution | Explicitly authorized Python and R Jupyter pages, structured and validated output, bounded cleanup, and a page cache | `src/execution/`; `tests/milestone_six.rs`, `tests/execution_cache.rs` |
| Portable handoff | Workspace assembly, reference resolution, and a self-contained SQLite snapshot with canonical text export | `src/assembly/`, `src/validation/`, `src/snapshots/`; `tests/snapshot_publication.rs`, `tests/monorepo_example.rs` |
| Site | Snapshot-backed routes, HTML, navigation, search, local assets, and atomic publication | `src/site.rs`, `src/rendering/`; `tests/site_generation.rs`, `tests/browser/site.spec.ts` |
| Commands | `check`, `extract`, `generate`, `build`, and watched `serve`, including optional live reload | `src/commands/`; `tests/commands.rs`, `tests/cli.rs` |

These are implemented paths, not a claim that every acceptance and release check
below has passed on a clean checkout. The
[architecture](DESIGN.md), [collaborator code map](docs/development/handoff.md),
[CLI guide](docs/guide/cli.md), and [acceptance matrix](tests/fixtures/acceptance/MATRIX.md)
give their respective contracts. The [R/Python example](examples/monorepo/README.md)
is the smallest complete site.

## MVP acceptance criteria

These stable IDs connect the [case registry](tests/fixtures/acceptance/CASES.json)
and [acceptance matrix](tests/fixtures/acceptance/MATRIX.md) to the release gate.
Each criterion still needs its complete scenario evidence before release.

- **MVP-01:** Explicit configuration for the full Python/R workspace.
- **MVP-02:** Static Python and R API extraction without running package code.
- **MVP-03:** In-process GFM and QMD parsing with visible unsupported syntax.
- **MVP-04:** Authorized, structured Python and R authored execution.
- **MVP-05:** One safe site renderer with navigation, concepts, links, and search.
- **MVP-06:** Correct behavior and failure boundaries for all five commands.
- **MVP-07:** Deterministic, portable snapshots and site output.
- **MVP-08:** No implicit installation, network access, or execution.
- **MVP-09:** Passing acceptance, quality, and end-to-end checks.
- **MVP-10:** Checked, previewed, and published self-documentation.
- **MVP-11:** Reproducible new-user and separate-stage workflows.

## Next handoff

The [code map](docs/development/handoff.md) describes the generator boundary.
The [monorepo integration test](tests/monorepo_example.rs) already copies a
snapshot, removes its source workspace, and compares the independently generated
site byte for byte with the original site.

- [ ] Produce a reviewed monorepo snapshot fixture from the real `extract`
  command, together with its canonical text export and example queries. Keep
  generated files out of the source fixture; the test should create them in a
  temporary directory.
  The [schema](docs/design/snapshot-schema.md) defines its wire format.

## Before the first release

### Product and acceptance

- [ ] Audit the [acceptance matrix](tests/fixtures/acceptance/MATRIX.md) against
  the current implementation. Confirm routes, navigation and visibility,
  semantic links, search, source links, output safety, and assets with focused
  tests. Update expectations only after inspecting the product behavior.
- [ ] Verify command equivalence and failure boundaries across the complete
  acceptance workspace: separate `extract` plus `generate` versus `build`,
  generation from a relocated snapshot, and preservation of the last successful
  snapshot and site after failures.
- [ ] Compare two builds from identical declared inputs in different absolute
  paths. Require byte-identical site trees and logically identical snapshot
  records and assets; SQLite file bytes need not match.
- [ ] Audit portable records, diagnostics, HTML, and search data for local path
  leaks and nondeterministic metadata. Confirm that actual R parser observations
  and execution toolchain and kernel observations populate provenance, closing
  the old Milestone 3 subtask. Run source and command tests with network access
  denied where the declared environment permits it.
- [ ] Complete browser review at desktop and mobile sizes, including keyboard
  navigation, focus, headings, labels, contrast, links, and search. Use
  `site-capture` and inspect its screenshots and failure traces.

### Project documentation

- [x] Finish the dogfooded user guide: installation, supported GFM and QMD
  syntax, Python and R extraction limits, diagnostics, snapshot compatibility,
  and the security model for unsandboxed authored execution. Keep user guidance
  separate from design decisions and dated test reports.
- [x] Add and verify an in-tree project-site check covering local links,
  fragments, assets, and search. Confirm that a content-only workspace works
  without API extraction targets and that generated outputs are outside watched
  inputs.
- [ ] Add the documentation publishing workflow. Build the project site on pull
  requests and `main`; deploy the Diplodocus-generated tree from version tags
  or an explicit manual dispatch to Cloudflare Workers at `diplodocus.cc`.

### Release

- [ ] Run the full Rust, browser, snapshot, execution, and documentation gates
  from a clean checkout in the declared `devenv` environment. Include
  `cargo fmt --all -- --check`,
  `cargo clippy --locked --all-targets --all-features -- -D warnings`,
  `cargo test --locked --all-targets`,
  `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps`,
  `site-capture`, and `cargo publish --locked --dry-run`.
- [ ] Verify that the Versionary release pull request, version tag, protected
  crate publishing, and documentation deployment trigger only as intended.
- [ ] Re-run the documented quick start from a clean checkout with declared
  tools installed. Verify the static example, the executing acceptance site,
  and independent generation from an exported snapshot.

## Scope held for later

Incremental extraction and rendering; older snapshot migrations; historical
release assembly; additional language extractors; automatic repository discovery;
external extractor, renderer, or execution plugins; whole-document notebooks;
cell-level caching; arbitrary themes and Quarto compatibility; and a hosted
service are outside the first release. Browser live reload is already implemented
as an optional `serve --live-reload` feature.

Change this page when a release gate changes. Keep detailed validation evidence
in tests and CI results rather than adding another dated ledger.
