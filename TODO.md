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
reference](docs/reference/cli.md), and [acceptance
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

## Eunoia.jl documentation integration

Use the local checkouts at `/home/jola/projects/Eunoia.jl` and
`/home/jola/projects/eunoia` as the acceptance case. Keep Julia docstrings and
guides in Eunoia.jl, and import their generated HTML and Markdown into Eunoia's
SvelteKit site. The existing integration is documented in
[Eunoia's Python docs guide](../eunoia/web/PYTHON-DOCS.md) and implemented in
[`python-docs.mjs`](../eunoia/web/scripts/python-docs.mjs).

The initial trial with Diplodocus 0.11.0 and Eunoia.jl 1.0.1 extracted all 23
exported Julia bindings. Fragment generation and host URL mapping worked after
isolating the authored-page reference failures. Complete documentation still
needs the work below; successful API extraction does not establish gallery or
Documenter compatibility.

### Diplodocus work

- [x] Add a real Eunoia.jl API acceptance recipe alongside
  `examples/eunoia/rust.py`. Cover all 23 exported bindings, `eunoiaplot!`,
  result types, fields, API references, and matching HTML and Markdown
  anchors. Verify generation from a relocated snapshot without the staged
  source checkout, configuration, or Julia runtime. Use disposable source
  copies for destructive portability checks.
- [ ] Extend the Eunoia.jl acceptance recipe to authored Documenter guides,
  gallery execution, and retained figures. Verify figure bytes and links in
  detached HTML and Markdown output after removing the disposable sources.
- [x] Fix Julia Markdown list conversion in
  `src/extractors/julia/docstrings.rs`. Convert inline markup once, preserve
  spaces in wrapped prose, and retain nested blocks in source order. Cover
  inline code, emphasis, links, API references, and exact source attribution
  with focused extraction and fragment regressions.
- [x] Support fenced code nested in Julia Markdown list items. Pin
  `fatou-parser` 0.9.0 for the upstream parser fixes, preserve separate code
  tokens and exact source attribution, and retain indented code blocks.
  Extraction and detached-fragment regressions run normally. The Eunoia recipe
  requires both examples to render as code in HTML and Markdown, with no
  literal fences in HTML list items.
- [x] Support the Documenter page syntax used in Eunoia.jl's `docs/src/`.
  Add `format: documenter` for Julia Markdown with implicit and explicit API,
  page, and heading `@ref` links and named heading anchors. Expand `@index`
  and `@autodocs`, including literal `Modules` and `Private = false`.
  Retain embedded API context and anchors in detached HTML and Markdown.
  Verify Eunoia's three pages, 23 public bindings, and mapped local links.
- [ ] Execute Documenter `@example` blocks with prepared Julia dependencies.
  Eunoia.jl has 30 gallery blocks and one homepage example; importing them as GFM
  produces no figures. Preserve named sessions within a page, anonymous example
  isolation, `# hide`, and native text and image output. Retain validated figures,
  execution observations, and environment inputs in portable snapshots. The
  existing native Julia adapter supports `jldoctest` and rejects `@example` and
  `@repl`; authored GFM does not execute them either.
- [ ] Define Julia API visibility for navigation and fragment consumers.
  Documented private helpers currently appear as visible API pages, unlike the
  existing `Private = false` reference. `Item::discoverable` does not apply Julia
  visibility. Preserve addressable references and fields of exported types when
  selecting the public surface.
- [ ] Improve Julia type presentation. Fields currently receive separate,
  mostly empty pages. Consider grouping them under their owning type with stable
  anchors and consistent HTML, Markdown, navigation, and URL mapping.
- [ ] Define support for explicitly supplied Julia package extensions. The
  current include traversal misses `ext/EunoiaMakieExt.jl`, so plotting pages
  show core fallback method signatures instead of the extension implementations.
  Preserve the existing generic documentation and avoid inventing methods from
  unavailable dependencies.
- [ ] Resolve the package-overview choice. Automatic README ingestion rejects
  Eunoia.jl's remote badge images. An authored package index can replace that
  fallback; determine whether remote-image support also belongs in Diplodocus.

### Eunoia integration follow-up

- [ ] Upgrade the existing Python integration when advancing the Diplodocus pin.
  Eunoia pins 0.2.0 and generates TOML with singular collection keys; current
  Diplodocus requires YAML with plural keys. Regenerate snapshots as part of the
  upgrade: the existing Python snapshot uses IR version 5, while 0.11.0 uses
  version 9 and rejects that snapshot.
- [ ] Prepare both package environments in local builds and CI. Fresh Python
  extraction now imports the checkout and fails without the built
  `eunoia._eunoia` extension. Julia extraction requires its package dependencies
  and native library; gallery execution additionally needs the environment in
  Eunoia.jl's `docs/Project.toml`, including CairoMakie. Supply the native library
  through a matching artifact or `EUNOIA_CAPI_LIB`.
- [ ] Generalize Eunoia's Python generator for pinned Julia sources, snapshots,
  assets, and routes. Its API selection currently assumes top-level Python
  names; support Julia's `!` names and type/member relationships. Preserve
  generated-directory ownership, atomic replacement, caching, and link checks.
  Add Julia navigation and update the reference overview, quickstart links, and
  `web/static/llms.txt`. Existing concrete-route sitemap discovery and
  `reference.md` export can include the generated pages automatically.

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
- [ ] Add Markdown includes for composing authored pages from existing files.
  Preserve each included file's source location for relative links, images,
  diagnostics, preview watching, and portable snapshots. Define nested
  includes and cycle detection before adding partial-file selection.

Change this page when a release gate changes. Keep detailed validation evidence
in tests and CI results rather than adding another dated ledger.
