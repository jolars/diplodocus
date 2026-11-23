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
::: {.component name="explorer" props='{"sets":{"A":5,"B":3,"A&B":2},"editable":true}'}

A static explanation, image, and links remain available without JavaScript.

:::
```

Evaluated R Markdown articles do not retain component blocks.

The optional `props` attribute takes a single-line JSON object. Use single
quotes around the attribute and write apostrophes inside JSON strings as
`\u0027`. Each mount receives its own props object, which defaults to `{}`.
An optional block ID remains available as an HTML anchor.

Set `loading` to show a plain-text message while the bundle loads and the
component mounts. Set `min-height` to reserve space while the component loads
and after it mounts. Use either attribute on its own or both together.

```markdown
::: {.component name="explorer" #explorer loading="Loading the explorer…" min-height="24rem"}

A static illustration of the explorer.

:::
```

The height accepts `0` or a nonnegative number in `px`, `em`, or `rem`.
For responsive sizing, set `--diplodocus-component-min-height` on the component's
anchor in a [site stylesheet](../presentation.md).
The fallback appears at its natural height when JavaScript is disabled or the
component fails to load.

The module exports `mount(element, { signal, assetBaseUrl, props })`. It may return a
cleanup function or a promise for one. Use the abort signal to stop workers,
remove listeners, and cancel animation. Use `new URL("worker.js", import.meta.url)`
for bundle-relative resources. Components initialize near the viewport on both
direct visits and client-side navigation. Stylesheets should scope their rules
to the component and use the theme's semantic CSS properties.

Components control their own table layout and styles. Diplodocus applies its
table styles to the tables it renders from documentation, including fallback
content. Those documentation tables support keyboard focus and scrolling.

Configuration-based builds enable their explicitly declared bundles. For a
trusted snapshot, `generate --allow-components` enables retained bundles. Without
that flag, generation emits only the fallback. Markdown and fragment exports
always retain the ordinary fallback content. This capability does not enable
raw HTML or executable notebook output.

See the [configuration reference](../../reference/configuration.md#components) for bundle fields.
