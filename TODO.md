# TODO

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
- [x] Allow per-page indexing directives in presentation metadata. Eunoia keeps
  some API routes available for compatibility and currently injects
  `noindex` into their generated HTML.
- [x] Make the Nix package link SQLite with the nixpkgs revision used by
  consumers. Eunoia still supplies SQLite through a local package override
  because its build fails to resolve `-lsqlite3`.
- [x] Identify which check raises `InputsChanged` and report the input path and
  expected and observed fingerprints when available. In Qualpal's
  cache-enabled build, Doxygen's package cache inventory sees two files the
  build itself changes. R discovery updates `qualpalr/DESCRIPTION` through
  roxygen2, and a Python API example writes `qualpal-py/palette.png`.

## Preview performance

- [x] Update edited Markdown pages in place in `diplodocus serve`. Use
  filesystem events to detect saves. Start with edits that preserve the page
  URL, title, and heading anchors. Validate and render the changed page, update
  its search entry, and swap its HTML into the open tab. Use the full rebuild
  for changes to shared inputs, routes, titles, or anchors. Keep the last good
  preview on failure and measure save-to-visible latency with `npm run site-bench`.
