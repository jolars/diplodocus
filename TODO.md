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

## Qualpal and qualpalr documentation integration

Use the local checkouts at `../qualpal` and `../qualpalr` as the acceptance case
for a shared C++ and R documentation site. Both API extractors, cross-repository
configuration, and explicit R/C++ API associations work. The existing
documentation still needs the compatibility fixes and migration work below
before it can build in full.

### Diplodocus work

- [x] Accept `\item{\dots}` argument labels in Rd documentation. Normalize
  `\dots` and `\ldots` to the maintained `...` formal, including grouped
  labels. Regression tests cover qualpalr's `plot.qualpal`, `pairs.qualpal`,
  and `print.qualpal` topics, preserved source, and unmatched formals.
- [x] Preserve argument documentation in standalone deprecated Rd topics without
  requiring callable formals. Qualpalr's `man/autopal-deprecated.Rd` retains
  its arguments, return values, prose, and help links through snapshot
  rendering. Maintained functions still require matching formals.
- [x] Preserve native `HAVE_DOT` during XML-only extraction so existing
  Doxyfiles with `WARN_AS_ERROR=YES` accept `@dot` blocks without invoking
  Graphviz. Cover the generated `build/Doxyfile.docs`, retain genuine
  extraction diagnostics, and document a headers-only Doxyfile as an
  alternative.
- [x] Support qualpal's static Matplotlib SVGs through a documented PNG
  conversion path. The converter retains styles, definitions, references,
  and clipping without scripts or external resource loading. All 15 guide
  image references validate after conversion in disposable copies, and
  detached generation retains the figures. Image validation failures now
  identify the asset and conversion remedy at the authored reference's
  source location.
- [x] Handle `LINE_PREFIX` as parser layout in authored Markdown. Wrapped list
  text in qualpalr's `NEWS.md`, `CONDUCT.md`, and qualpal's guides retains
  continuation spacing and nested structure without warnings, unsupported
  HTML spans, or whitespace-only code in Markdown exports. Regression tests
  cover GFM, QMD, R Markdown, CRLF, fragments, and rendering.
- [ ] Add a complete qualpal/qualpalr acceptance recipe. Cover C++ namespaces,
  types, templates, overloads, R help topics and S3 methods, executed Rd
  examples and articles, explicit API associations, and retained figures.
  Verify detached generation from a relocated snapshot without either
  checkout, Doxygen, or R. Keep all trial work in disposable source copies.

### Qualpal integration follow-up

- [ ] Configure one site with both repositories, shared guides, separate C++ and
  R reference sections, explicit API concepts, and the R package's binding
  relationship to the C++ library. Choose package overviews and navigation
  explicitly rather than relying on the existing site layouts.
- [ ] Migrate qualpal's Doxygen guides into content collections. Replace
  document-wide `#page-id` links and Doxygen API links with supported page
  and semantic references, convert `\f$...\f$` math, and replace the `@dot`
  color-space diagram with a supported static figure. Standalone Doxygen
  pages, groups, and generated diagrams are not currently imported.
- [ ] Replace or support the inline `<sub>ab</sub>` markup in
  `vignettes/comparisons.Rmd`. It currently causes rendered R Markdown to
  fail, although source checking reports only a warning. Preserve the
  subscript in the published article.
- [ ] Choose how to publish the introduction's four `rglwidget()` outputs.
  Provide static figures that work in the prepared build environment, or add
  explicit widget support with retained browser dependencies and portable
  snapshots. The current Markdown path attempts an OpenGL snapshot and fails
  in the supplied headless R environment.
- [ ] Preserve published URLs using `qualpal/docs/site-contract.json` as the
  compatibility fixture. Map its 61 legacy paths and recorded anchors to the
  new site, including member and source pages, and redirect qualpalr's old
  site. Handle fragment aliases and redirects between origins separately:
  `presentation.redirects` currently supports neither.
- [ ] Replace the documentation build and deployment workflows with a shared
  Diplodocus build under `/docs/`. Prepare both checkouts at selected
  revisions, Doxygen, R dependencies, and qualpalr's compiled extension
  before extraction. Include article dependencies such as `maps` and
  `randtoolbox`, plus the chosen 3D rendering prerequisites. Keep
  `README.Rmd` generation as a separate knitr step while needed, and
  validate links, assets, and legacy URLs before deployment.

## Eunoia site and native preview integration

Use Eunoia's `web/scripts/site-output.mjs` as the acceptance case for site
features that currently require rewriting generated output. Support these
features through declarations shared by `build`, `generate`, and `serve`, with
settings and asset bytes retained in portable snapshots. Keep widget compilation
with external tools such as Vite, following the browser component contract.

- [x] Declare public site assets and icons in configuration. Retain files such
  as favicons, web manifests, and `llms.txt` even when no authored page
  references them. Preserve their publication paths, watch source changes
  during `serve`, and validate icon references and collisions with generated
  output. Verify equivalent assets in native preview and generation from a
  relocated snapshot without the source checkout.
- [x] Add per-page descriptions and title overrides for generated package and
  API pages. Define precedence with site defaults and existing authored
  titles, and apply resolved metadata consistently to HTML, Open Graph, and
  Twitter cards. Define how title overrides affect headings, navigation, and
  search. Verify that detached generation and native preview use the same
  metadata.
- [x] Support explicit legacy fragment aliases attached to maintained headings
  or API targets. Combine aliases with existing route redirects, preserve
  canonical anchors, and retain aliases in HTML and Markdown output.
  Diagnose missing targets and duplicate IDs. Use Eunoia's
  `web/legacy-routes.json` and Qualpal's `docs/site-contract.json` as
  compatibility fixtures, and verify aliases in detached generation and
  native preview.
- [x] Add optional browser opening to `serve`, for example through `--open`.
  Open the actual local URL after the first successful build and server
  readiness, and avoid opening a browser when startup fails. Keep the
  command usable in noninteractive sessions, and test browser invocation
  without launching a desktop browser.

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
  - [x] Define the [authoring contract](docs/design/markdown-includes.md):
    VitePress syntax for GFM, Quarto syntax for QMD, paths relative to each
    included file, collection-root paths, repository boundaries, and errors
    for missing or invalid inputs.
  - [ ] Implement whole-file and nested includes with cycle detection. Retain
    each included file's source path and line locations so diagnostics can
    identify the included source and its include chain.
  - [ ] Resolve relative links and images from the included file's location,
    including files outside the including page's directory.
  - [ ] Track included files as build and preview dependencies. Rebuild affected
    pages when an included file changes, is created, or is removed.
  - [ ] Preserve expanded content, source locations, and referenced assets in
    portable snapshots so `generate` works without the original source tree.
  - [ ] Add partial-file selection after whole-file inclusion works: line
    ranges, named regions, and heading anchors, with diagnostics for invalid
    or missing selections and source locations retained after selection.
  - [ ] Support verbatim includes inside code fences, preserving indentation and
    allowing line-range selection without parsing the included code as
    Markdown.
  - [ ] Document the supported syntax and differences from VitePress and Quarto,
    and add integration coverage for nested includes, source-relative
    resources, preview updates, and generation from portable snapshots.

Change this page when a release gate changes. Keep detailed validation evidence
in tests and CI results rather than adding another dated ledger.
