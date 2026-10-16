# Watched package documentation in VitePress

This example uses the existing [Tiny Stats workspace](../monorepo/) to publish
Python, R, and Julia documentation alongside guides written for VitePress.
Diplodocus watches the package documentation and publishes portable SQLite
snapshots. A small host adapter exports Markdown fragments and retained assets
into VitePress's source tree. VitePress supplies the layout, navigation, outline,
local search, and development server.

## Run

From the Diplodocus repository root, use the pinned environment and install the
frontend dependencies:

```console
devenv shell
npm ci --ignore-scripts
npm run vitepress-dev
```

Open <http://127.0.0.1:5173/>. The command builds Diplodocus, starts
`extract --watch`, waits for the first completed reference, and starts VitePress.
Use `-- --port 5174` to choose another port. `DIPLODOCUS_BIN` selects an already
built executable instead of running Cargo.

Edit `examples/vitepress/docs/guide.md` to see VitePress update the guide. Edit
`examples/monorepo/python/src/tinystats/__init__.py`, the R package's Rd files,
or the Julia source to see extraction and the API reference update. Adding or
removing a public API also updates its page and the reference navigation. Ctrl-C
stops both processes and waits for execution cleanup.

Guide edits do not run package extraction. Package rebuilds prepare a complete
generation before publication, then briefly restart VitePress so its routes,
outline, sidebar, and search use the new reference. The browser reconnects and
reloads. This example deliberately starts with a full refresh for package edits;
more selective hot updates can be added without changing the snapshot contract.

## Build

```console
npm run vitepress-build
```

This runs extraction, fragment export, host adaptation, and `vitepress build` in
that order. The static site is in `examples/vitepress/docs/.vitepress/dist/`.
For generation without the package sources or runtimes, provide a snapshot:

```console
npm run vitepress-build -- --snapshot /path/to/documentation.sqlite
```

CI can run the same build after either guides or package sources change. With
separate source repositories, the caller supplies the checkouts and triggers the
site build when a selected package revision changes.

## Adapter boundary

`run.mjs` consumes the versioned JSON publication events on `extract --watch`'s
stdout. Diagnostics remain on stderr. It copies each completed snapshot before
discovering documents, assigning host URLs, and exporting fragments with the
existing URL-map interface. Each export therefore uses one coherent snapshot,
even if extraction publishes again during adaptation.

The adapter turns retained heading anchors into VitePress custom heading IDs.
Generated Markdown participates in native routing, search, and outlines. All
snapshot documents remain available as link targets; visibility controls API
navigation and search. The example uses package IDs from Tiny Stats to choose
its Python, R, and Julia mounts. This is example host policy, not a framework
plugin API.

Snapshots and staging directories live in `.diplodocus/`. The generated
`docs/reference/` and `docs/public/_diplodocus/` directories carry ownership
markers; the adapter refuses to replace author-written directories. It stages
both trees, replaces the previous generated output, and removes obsolete pages.
Failed extraction keeps the previous reference available while authored guides
can still update. Initial extraction or adaptation failures stop startup, and
production failures stop the build.

The example pins VitePress 1.6.4. The root npm overrides select patched Vite
6.4.3 instead of the older Vite release required by VitePress's default range.

## Verify

```console
npm run vitepress-test
```

The browser check uses temporary copies of both examples. It checks guide and
docstring edits, added and removed APIs, cross-language links, stable anchors,
failure recovery, local search, and desktop and mobile layouts. It then removes
the package sources and builds from the retained snapshot. Screenshots, a browser
trace, and the development log go to `artifacts/vitepress/`; set
`DIPLODOCUS_VITEPRESS_ARTIFACTS` to choose another directory.
