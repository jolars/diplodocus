# Project status and remaining work

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
  - [x] Define the [include behavior](docs/guide/authoring.md#include-shared-content):
    VitePress syntax for GFM, Quarto syntax for QMD, paths relative to each
    included file, collection-root paths, repository boundaries, and errors
    for missing or invalid inputs.
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
