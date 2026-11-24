# Project status and remaining work

## Scope held for later

The roadmap above sets implementation order; it does not make every stage a
first-release gate. Older snapshot migrations; historical release assembly;
additional language extractors; automatic repository discovery; external
extractor, renderer, or execution plugins; whole-document notebooks; cell-level
caching; arbitrary themes and full Quarto compatibility; and a hosted service
remain outside the first release. Browser live reload is already implemented as
an optional `serve --live-reload` feature.

- [x] Support [browser components](docs/guide/authoring/components.md) in authored
  GFM and QMD pages, with per-instance JSON props, Markdown fallbacks, and
  prebuilt bundles retained in portable snapshots.
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
  - [x] Support verbatim includes inside code fences, preserving indentation and
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

## Gaps found in downstream sites

- [x] Let Rust source discovery skip unrelated generated files before resolving
  symlinks. Keep repository-boundary checks for declared inputs and files
  reported by rustdoc. Shrinkage patches the inventory and LazyMatrix
  exports a separate source tree to avoid Nix-generated links.
- [x] Support site-wide stylesheets and scripts backed by declared public
  assets. Include them in preview, snapshots, and generated sites so Eunoia,
  Datamonger, Shrinkage, and the Diplodocus docs can drop their head
  rewrites.
- [x] Publish public asset directories with a declared destination prefix. Check
  collisions and track membership changes so Datamonger can publish its
  generated catalog JSON through the regular build and preview.
- [x] Let browser components declare a loading state and reserve layout space
  before their bundle mounts. Preserve the authored fallback without
  JavaScript. Shrinkage currently inserts its explorer loading indicator
  after rendering.
- [x] Expose the document and asset inventory for fragment generation before
  rendering. External frontends could build a URL map without generating the
  fragments twice.
- [x] Resolve relative `srcset` candidates against their source page before
  client-side navigation updates the base URL. Cover retained header images
  and navigation between nested routes so Eunoia can drop its rewrite.
- [ ] Allow per-page indexing directives in presentation metadata. Eunoia keeps
  some API routes available for compatibility and currently injects
  `noindex` into their generated HTML.
- [ ] Make the Nix package link SQLite with the nixpkgs revision used by
  consumers. Eunoia still supplies SQLite through a local package override
  because its build fails to resolve `-lsqlite3`.
- [ ] Identify which check raises `InputsChanged` and report the input path
  and expected and observed fingerprints when available. Qualpal's
  cache-enabled production build failed in an isolated checkout, while
  `build --no-cache` and a cold-cache `serve` passed. Use the diagnostics to
  determine whether the build observes a real input change or invalidates its
  own inputs.
