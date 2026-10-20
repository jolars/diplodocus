# Project status and remaining work

## What works today

  | Area               | Current implementation                                                                                                                                                | Evidence to start with                                                                    |
  | ------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
  | Workspace input    | Explicit repository, package, content, concept, relationship, and execution configuration; Python, R, Julia, and compiler-backed Rust extraction; GFM and QMD parsing | `src/configuration.rs`, `src/extractors/`, `src/documents/`; `tests/acceptance_gate.rs`   |
  | Authored execution | Explicitly authorized Python and R Jupyter pages, structured and validated output, bounded cleanup, and a page cache                                                  | `src/execution/`; `tests/milestone_six.rs`, `tests/execution_cache.rs`                    |
  | Site               | Snapshot-backed routes, HTML, navigation, search, local assets, and atomic publication                                                                                | `src/site.rs`, `src/rendering/`; `tests/site_generation.rs`, `tests/browser/site.spec.ts` |
  | Commands           | `check`, `extract`, `generate`, `build`, and watched `serve`, including optional live reload                                                                          | `src/commands/`; `tests/commands.rs`, `tests/cli.rs`                                      |

These are implemented paths, not a claim that every acceptance and release check
below has passed on a clean checkout. The [architecture](DESIGN.md), [CLI
guide](docs/guide/cli.md), and [acceptance
matrix](tests/fixtures/acceptance/MATRIX.md) give their respective contracts.
The [four-language example](examples/monorepo/README.md) is the smallest
complete site.

The accepted [prepared-environment
contract](docs/decisions/0003-prepared-environment.md) changes the intended
extraction behavior. Julia runtime type completion and required Python, R, and
Julia API-example execution are implemented. Package loading is not yet required
for every extraction, and QMD execution still requires explicit configuration.

## MVP acceptance criteria

These stable IDs connect the [case
registry](tests/fixtures/acceptance/CASES.json) and [acceptance
matrix](tests/fixtures/acceptance/MATRIX.md) to the release gate. Each criterion
still needs its complete scenario evidence before release.

- **MVP-01:** Explicit configuration for the full Python/R workspace.
- **MVP-02:** Python and R API extraction in caller-prepared package
  environments, with source analysis and runtime assistance as needed.
- **MVP-03:** In-process GFM and QMD parsing with visible unsupported syntax.
- **MVP-04:** Required execution of recognized API and authored examples, with
  structured results, expected-output verification, and explicit recorded skips.
- **MVP-05:** One safe site renderer with navigation, concepts, links, and
  search.
- **MVP-06:** Correct behavior and failure boundaries for all five commands.
- **MVP-07:** Deterministic, portable snapshots and site output.
- **MVP-08:** No implicit installation, dependency resolution, package builds,
  or network access by Diplodocus. Extraction authorizes package loading and
  example execution; `check` does not execute package code, and `generate`
  requires no package environment.
- **MVP-09:** Passing acceptance, quality, and end-to-end checks.
- **MVP-10:** Checked, previewed, and published self-documentation.
- **MVP-11:** Reproducible new-user and separate-stage workflows.

## Before the first release

### Product and acceptance

The [prepared-environment contract](docs/decisions/0003-prepared-environment.md)
still needs the tasks below. Keep package loading and example execution out of
`check`; Rust retains its documented compiler and build-script exception.

- [x] Validate and load every Python checkout during extraction, including
  packages without doctests. Reuse the checkout loader and verify the loaded
  package's source path and version independently of example discovery. Add
  command tests without doctests for a missing interpreter, missing
  dependencies, and an incorrect checkout; failures must preserve the
  previous snapshot and site.
- [x] Validate and load every Julia checkout during extraction, including
  packages without unresolved public bindings or doctests. Verify the
  package name, UUID, version, and entry file independently of reflection
  and example discovery. Add command tests without those triggers for a
  missing runtime, missing dependencies, an incorrect checkout, and
  package-load errors; failures must preserve the previous snapshot and
  site.
- [ ] Validate and load every R checkout during extraction, including packages
  without Rd examples or R Markdown vignettes. Reuse `pkgload` with
  compilation disabled, verify the namespace identity and version, and
  require prebuilt extensions. Add command tests without examples or
  vignettes for missing dependencies and extensions, an incorrect checkout,
  and failing load hooks; failures must preserve the previous snapshot and
  site.
- [ ] Define and implement Python's supported source/runtime reconciliation
  rules. State which API facts remain authoritative in source and stubs,
  retain runtime observations as separate evidence, and diagnose
  contradictions in the supported subset. Cover agreement and conflicts
  while keeping source validation available without importing the package
  through `check`.
- [ ] Define and implement R's supported source/runtime reconciliation rules.
  State how namespace, source, and Rd facts relate to runtime observations,
  preserve their separate attribution, and diagnose contradictions in the
  supported subset. Cover agreement and conflicts while keeping `check` free
  of package loading.
- [ ] Extend Julia source/runtime reconciliation beyond the existing type-flavor
  check. Define supported comparisons for type parameters, supertypes, and
  fields, and diagnose incompatible overlapping observations. Test conflicts
  and preserve source evidence separately from runtime evidence without
  inventing source ranges for generated declarations.
- [ ] Execute recognized API examples and authored cells during extraction
  without a workspace opt-in. Define language-specific setup, session state,
  expected-output comparison, expected errors, illustrative syntax, and
  recorded conditional skips. Diagnose unsupported executable syntax and
  unavailable engines instead of leaving examples unchecked.
- [ ] Extend provenance, cache identities, and snapshot validation for runtime
  observations and API-example outcomes, outputs, and assets. Select any
  required schema and execution-policy version changes before publishing new
  records.
- [ ] Replace source-only acceptance packages with usable checkout environments,
  including compiled extensions. Keep parser-only fixtures for component
  tests. Update the case registry and matrix as implementation lands, and
  cover missing prerequisites, incorrect package identity, example errors
  and output mismatches, explicit skips, failure preservation, and
  runtime-free generation.
- [ ] Update configuration, language references, guides, examples, and CI
  environments alongside implementation. Remove obsolete execution opt-in
  and runtime-free extraction promises only when the new behavior is
  available.
- [ ] Audit the [acceptance matrix](tests/fixtures/acceptance/MATRIX.md) against
  the current implementation. Confirm routes, navigation and visibility,
  semantic links, search, source links, output safety, and assets with
  focused tests. Update expectations only after inspecting the product
  behavior.
- [ ] Verify command equivalence and failure boundaries across the complete
  acceptance workspace: separate `extract` plus `generate` versus `build`,
  generation from a relocated snapshot, and preservation of the last
  successful snapshot and site after failures.
- [ ] Compare two builds from identical declared inputs in different absolute
  paths. Require byte-identical site trees and logically identical snapshot
  records and assets; SQLite file bytes need not match.
- [ ] Audit portable records, diagnostics, HTML, and search data for local path
  leaks and nondeterministic metadata. Confirm that actual R parser
  observations and execution toolchain and kernel observations populate
  provenance, closing the old Milestone 3 subtask. Run source and command
  tests with network access denied where the declared environment permits
  it.
- [x] Add persistent client-side navigation for internal page links. Keep the
  shared navbar, branding, and logo mounted while replacing page content and
  updating navigation state, so links do not trigger full page reloads or
  logo flicker. Preserve directly accessible static HTML and navigation
  without JavaScript. Handle browser history, back and forward navigation,
  page titles, anchors, scroll restoration, focus, and page script
  initialization. Add browser tests for these behaviors and verify that the
  logo and navigation remain visually stable between pages.
- [ ] Complete browser review at desktop and mobile sizes, including keyboard
  navigation, focus, headings, labels, contrast, links, and search. Use
  `site-capture` and inspect its screenshots and failure traces.

## Distribution and installation

Prioritize versioned prebuilt releases and a setup action, followed by PyPI. Add
npm as a secondary route for projects with documentation frontends. Publish the
same CLI version across channels; callers still prepare their language runtimes
and package dependencies.

- [x] Implement a separate `setup-diplodocus` GitHub Action that accepts a
  version, downloads the appropriate release binary, and adds it to `PATH`.
  Keep documentation builds as ordinary CLI steps in the caller's prepared
  environment.
- [ ] Publish the CLI on PyPI as prebuilt wheels using Maturin's binary
  packaging support. Document adding Diplodocus to a documentation
  dependency group and running it with `uv run --group docs` so projects can
  lock its version alongside their dependencies. Explain when extraction and
  example execution need the project environment rather than an isolated
  `uvx` environment.
- [ ] Publish an npm package for use as a documentation development dependency
  and through `npx`. Support `npm ci --ignore-scripts`, as used by
  Datamonger and Shrinkage, by packaging platform binaries without requiring
  a postinstall download.

## Scope held for later

Incremental extraction and rendering; older snapshot migrations; historical
release assembly; additional language extractors; automatic repository
discovery; external extractor, renderer, or execution plugins; whole-document
notebooks; cell-level caching; arbitrary themes and Quarto compatibility; and a
hosted service are outside the first release. Browser live reload is already
implemented as an optional `serve --live-reload` feature.

- [ ] Add first-class interactive component embedding for authored pages.
  Diplodocus should provide authoring syntax for declaring a component and
  its inputs, manage mounting and asset loading, and preserve the complete
  bundle in portable snapshots so `generate` works without the original
  sources. External tools should compile components built with Svelte,
  TypeScript, WASM, or other browser technologies; Diplodocus should define
  the embedding contract rather than build its own frontend framework or
  bundler. Support JavaScript, CSS, WASM, workers, and related assets while
  preserving bundle paths. Use Eunoia's integrated diagram app and inline
  documentation examples as motivating cases, with a small browser runtime
  or adapters where useful.

Change this page when a release gate changes. Keep detailed validation evidence
in tests and CI results rather than adding another dated ledger.
