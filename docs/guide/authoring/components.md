# Browser components

Use a browser component when a page needs a custom interactive tool. Build its
JavaScript and stylesheets before building the documentation, and provide
fallback content for readers who do not run JavaScript.


Declare a prebuilt bundle in the workspace configuration:

```yaml
components:
  - id: explorer
    repository: core
    path: docs/explorer/dist
    module: index.js
    stylesheets: [style.css]
```

The bundle directory is relative to its declared repository. All files must be
regular files inside that boundary; symlinks are rejected. Diplodocus retains
the complete tree so relative module, worker, and WebAssembly URLs keep working.
Build the bundle before running `check`, `build`, `extract`, or `serve`.

Mount it in a GFM or QMD source:

```markdown
::: {.component name="explorer"}

A static explanation, image, and links remain available without JavaScript.

:::
```

Evaluated R Markdown articles do not retain component blocks.

The module exports `mount(element, { signal, assetBaseUrl })`. It may return a
cleanup function or a promise for one. Use the abort signal to stop workers,
remove listeners, and cancel animation. Use `new URL("worker.js", import.meta.url)`
for bundle-relative resources. Components initialize near the viewport on both
direct visits and client-side navigation. Stylesheets should scope their rules
to the component and use the theme's semantic CSS properties.

Configuration-based builds enable their explicitly declared bundles. For a
trusted snapshot, `generate --allow-components` enables retained bundles. Without
that flag, generation emits only the fallback. Markdown and fragment exports
always retain the ordinary fallback content. This capability does not enable
raw HTML or executable notebook output.

See the [configuration reference](../../reference/configuration.md#components) for bundle fields.
