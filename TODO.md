# Project status and remaining work

## Native API extraction roadmap

Use established language tooling as the authority for API semantics. Diplodocus
owns the common documentation model, cross-language references, snapshots,
navigation, search, rendering, and development loop. Keep Rust parsers for
source locations, change detection, and supported supplemental analysis rather
than reimplementing every language's runtime and documentation conventions.

The intended workflow is one command: `diplodocus serve`. For a package using
roxygen2, saving R source should make Diplodocus invoke roxygen2, consume the
generated documentation, rebuild affected pages, and refresh the browser. The
caller supplies the language tools, dependencies, and compiled extensions;
Diplodocus orchestrates them without installing dependencies.

R extraction now selects roxygen2 generation or maintained Rd and combines
documentation with native checkout discovery. Python uses Sphinx autodoc and
Napoleon with maintained stubs; Julia uses static source adapters with runtime
assistance. Rust and C++ consume native
rustdoc JSON and Doxygen XML.
The development loop reuses verified extraction and execution results by package.
Workspace reference resolution and site rendering still run after each change.
Keep the existing Rust and C++ integrations and use the [current
design](docs/development/design.md) as the migration baseline. Preserve native
execution engines such as knitr for authored documents; this roadmap does not
require replacing Quarto, MyST, or R Markdown in full.

### 1. Define the adapter boundary and measure the current pipeline

- [x] Define a common native adapter contract for package metadata, API items,
  signatures, documentation, symbolic references, source evidence,
  diagnostics, tool versions, and observed inputs. Keep site routing and
  rendering outside the language tools.
- [x] Specify prerequisites and command behavior for each adapter. `check` may
  load packages and invoke native extraction but must not execute
  documentation examples. `extract`, `build`, and `serve` must use the same
  API discovery rules and retain completed execution evidence before
  publication.
- [x] Inventory existing static capabilities and gaps against representative
  packages. Reuse Qualpal and qualpalr for R/C++ integration and add Python
  and Julia cases with generated APIs. Preserve supported identities,
  signatures, links, and source evidence during migration.
- [x] Measure startup and warm saves separately, including package loading,
  documentation generation, extraction, examples, reference resolution, and
  rendering. Compare baseline and candidate in one `compute bench` window
  and choose measurable preview targets from the results.

The [adapter contract and capability inventory](docs/development/native-adapters.md)
record the shared boundary and migration gaps. The
[pipeline baseline](docs/development/pipeline-baseline.md) describes the
developer measurement workflow.

### 2. Deliver the R workflow with roxygen2

- [x] Select roxygen2 generation or maintained Rd input per package. Respect the
  package's supported roxygen options, roclets, and loading strategy, and
  document the supported versions and extension surface.
- [x] Invoke roxygen2 from the normal command pipeline when its inputs change.
  Users must not need a separate `devtools::document()` command before
  running `check`, `extract`, `build`, or `serve`.
- [x] Define where generated Rd and namespace metadata live and how they map
  back to maintained R sources. Document any checkout writes, prevent
  generated output from triggering watch loops, and reject stale output
  after generation fails.
- [x] Combine generated Rd with native exports, formals, and dispatch evidence.
  Stop requiring every documented declaration to be reconstructible from a
  direct static source definition. Retain source parsers for attribution and
  supported supplemental evidence.
- [x] Reuse native process supervision, checkout verification, cancellation,
  deadlines, diagnostics, and preservation of the previous successful site.
  Keep Rd example execution and portable snapshots working with generated
  input.
- [x] Add acceptance cases for roxygen prose, changed formals and exports,
  inherited documentation, shared topics, and generated declarations. In the
  qualpalr recipe, verify that plain `diplodocus serve` updates
  documentation after a source edit without manually regenerating Rd, and
  recovers after a generation error is fixed.

The [R workflow](docs/guide/extraction/r.md) documents configuration and native
prerequisites. The [qualpalr comparison](docs/development/pipeline-baseline.md#roxygen2-workflow-results)
records source-only preview updates, failure recovery, and current latency.

### 3. Make the development loop incremental

- [x] Make watching, native regeneration, and browser refresh work together
  through plain `diplodocus serve`, with documented defaults and overrides.
  Build on the existing watcher and optional live reload.
- [x] Start with package-level invalidation. Track source, documentation
  inheritance, configuration, tool versions, dependencies, and declared
  environment inputs; invalidate the whole affected package when
  dependencies are uncertain. Avoid re-extracting unrelated packages.
- [x] Reuse extraction or execution results only when their tracked inputs and
  environment justify it. Define separate policies for documentation
  generation and example execution; a comment-only edit is not proof that
  evaluated documentation or inherited topics are unchanged.
- [x] Rebuild affected documents and update references, navigation, and search
  when their dependencies change. Remove deleted items and assets, and
  compare incremental results with a clean build.
- [x] Measure representative R saves against the baseline before adding finer
  invalidation or persistent workers. If native extraction dominates,
  evaluate upstream improvements or an explicitly provisional preview with
  pending validation visible; keep completed snapshots and published builds
  verified.

The [incremental design](docs/development/design.md#incremental-work-retains-verified-results)
describes persistent reuse and fresh input guards. The
[comparison](docs/development/pipeline-baseline.md#incremental-workflow-results)
records authored-save improvements, cold-build and API-edit costs, and verified
restart parity.

### 4. Migrate Python through supported native interfaces

- [x] Prototype a Python exporter using the prepared package and Sphinx/autodoc.
  Identify the smallest supported interface that returns structured API
  records, including signatures and docstrings, rather than importing a
  separately rendered site. Define supported docstring formats, extension
  behavior, and precedence for maintained stubs.
- [x] Migrate Python commands with acceptance cases for existing public APIs
  and runtime-derived declarations. Preserve aliases, overloads, maintained
  stubs, source evidence, doctests, and detached generation. Keep
  unsupported behavior explicit rather than silently switching to a
  different API interpretation.
- [x] Update environment requirements, CI, configuration guidance, and extractor
  contracts alongside the migration. Remove superseded static semantic
  rules after the native path covers their acceptance cases; retain useful
  parsers.

The [Python extraction contract](docs/ir/python-extraction.md) describes native
discovery, maintained stubs, supported docstrings, and portable evidence. The
[comparison](docs/development/pipeline-baseline.md#python-native-results) records
startup, source-save, and verified restart costs.

### 5. Migrate Julia through supported native interfaces

- [ ] Prototype Julia extraction from the loaded package's documentation and
  method systems, using Documenter where needed. Define the supported export
  interface and compatibility policy before relying on Documenter internals.
  Cover generated declarations, attached documentation, and method identities.
- [ ] Migrate Julia commands with acceptance cases for existing public APIs and
  runtime-derived declarations. Preserve aliases, methods, source evidence,
  doctests, and detached generation. Keep unsupported behavior explicit rather
  than silently switching to a different API interpretation.
- [ ] Update environment requirements, CI, configuration guidance, and extractor
  contracts alongside the migration. Remove superseded static semantic rules
  after the native path covers their acceptance cases; retain useful parsers.

### 6. Compose documentation produced in separate repositories

- [ ] Define a versioned package API bundle built on the existing IR and
  snapshot machinery. Retain semantic identities, symbolic references,
  assets, execution evidence, tool versions, and source revisions before
  final site routing. Distinguish bundles from completed workspace
  snapshots.
- [ ] Import local bundles alongside locally extracted packages. Resolve
  cross-package references and choose routes after composition, with
  diagnostics for conflicting identities, incompatible formats, or missing
  dependencies. Verify relocation and generation without producer checkouts
  or runtimes.
- [ ] Add producer CI recipes that export revision-pinned bundles in each
  repository's own environment. Use Qualpal and qualpalr to demonstrate a
  shared site whose final assembly does not require the C++ producer's
  toolchain in the R repository.
- [ ] Add explicit remote bundle retrieval, caching, and mounting under a chosen
  site path, starting with one documented artifact source. Pin revisions and
  artifact digests and allow a local checkout to replace a published bundle
  during development. Repository shorthand must explain the producer
  artifact requirement; arbitrary source repositories still need extraction
  somewhere.
- [ ] Document and test both workflows: prepared local checkouts for active
  development, and independently published bundles for site assembly. Keep
  existing cross-repository configuration working throughout the migration.

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

## Scope held for later

The roadmap above sets implementation order; it does not make every stage a
first-release gate. Older snapshot migrations; historical release assembly;
additional language extractors; automatic repository discovery; external
extractor, renderer, or execution plugins; whole-document notebooks; cell-level
caching; arbitrary themes and full Quarto compatibility; and a hosted service
remain outside the first release. Browser live reload is already implemented as
an optional `serve --live-reload` feature.

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
  - [x] Define the [include
    behavior](docs/guide/authoring.md#include-shared-content): VitePress
    syntax for GFM, Quarto syntax for QMD, paths relative to each included
    file, collection-root paths, repository boundaries, and errors for
    missing or invalid inputs.
  - [x] Implement whole-file and nested includes with cycle detection. Retain
    each included file's source path and line locations so diagnostics can
    identify the included source and its include chain.
  - [x] Resolve relative links and images from the included file's location,
    including files outside the including page's directory.
  - [x] Fingerprint successfully resolved includes as build inputs, revalidate
    their contents, and expose their paths to the existing preview watcher.
  - [x] Retain watch dependencies after failed include resolution so creating or
    restoring a missing partial triggers another build. Add preview tests
    for editing, creating, deleting, and restoring nested partials,
    including files outside the collection directory.
  - [x] Retain expanded Markdown and parsed content in the document records
    consumed by portable snapshots.
  - [x] Retain original source maps and included-file-relative assets in
    portable snapshots. Add a regression test that removes the source tree
    before loading a snapshot and generating a site with nested includes.
  - [x] Preserve collection execution authority and document vetoes for included
    QMD cells.
  - [x] Integrate expanded QMD with execution identity, input verification,
    caching, and execution diagnostics before enabling executable pages with
    includes. Static expansion must not bypass the engine's source checks.
  - [ ] Add partial-file selection after whole-file inclusion works: line
    ranges, named regions, and heading anchors, with diagnostics for invalid
    or missing selections and source locations retained after selection.
  - [ ] Support verbatim includes inside code fences, preserving indentation and
    allowing line-range selection without parsing the included code as
    Markdown.
  - [x] Document the syntax and compatibility decisions in the include contract,
    and add integration tests for nested and repeated includes, cycle
    detection, source attribution, invalid targets, and repository
    boundaries.
  - [ ] Add authoring documentation once relative resources, preview recovery,
    and portable snapshots work. Document supported syntax and differences
    from VitePress and Quarto, and cover source-relative resources in
    integration tests alongside the preview and snapshot tests above.

Change this page when a release gate changes. Keep detailed validation evidence
in tests and CI results rather than adding another dated ledger.
