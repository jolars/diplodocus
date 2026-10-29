# Authored pages

Declare each source directory or individual file as a collection in `content`
in [workspace configuration](configuration.md). Set `format: "gfm"` for `.md` files or
`format: "qmd"` for `.qmd` files, or `format: "rmd"` for R Markdown articles.
Use `format: "documenter"` for Julia Markdown pages with Documenter references
and directives. A collection's `mount` determines its URL prefix; an empty mount
places its `index.md` at the site root. The `owner`
determines whether pages appear in project or package navigation.

Individual files keep their basename beneath the mount, so `CHANGELOG.md` with
`mount: project` becomes `/project/CHANGELOG.html`. Links and images resolve
from the original source directory.

## Documenter pages

Use `format: documenter` for Julia Markdown `.md` pages that use Documenter
references and API assembly directives:

```yaml
content:
  - id: julia-manual
    owner: julia
    repository: core
    path: julia/docs/src
    mount: ""
    format: documenter
```

The owner names a configured package, or `project`. A package-owned collection
with an empty mount uses its authored `index.md` as the package overview.
Diplodocus parses Julia Markdown with Fatou and retains heading levels, inline
formatting, lists, admonitions, tables, code, and source ranges. Titles come
from page headings. This profile does not read `docs/make.jl`.

References use Documenter's link syntax. `` [`euler`](@ref) `` and
`` [fit a diagram](@ref `Eunoia.euler`) `` resolve canonical API items.
Legacy `[fit a diagram](@ref Eunoia.euler)` also works.

`[API Reference](@ref)` infers a heading title from the visible label.
`[gallery](@ref "A gallery of Euler and Venn diagrams")` supplies a title
explicitly. Heading references search the containing collection, including
other pages, and require a unique match.

`## [Installation](@id install)` assigns a heading anchor. Link to it with
`[setup](@ref install)`. Ordinary file and fragment links also work.

API names use the owning package before requiring a unique workspace match.
References can address private items and exported type fields, regardless of
which items an assembly directive selects. Plain labels select headings;
backticked labels select API items and fall back to headings when no API item
matches. Explicit quoted targets select heading titles. Legacy unquoted
references prefer a matching heading over an API item. See
[Documenter's syntax reference](https://documenter.juliadocs.org/stable/man/syntax/)
for the upstream forms.

An `@autodocs` block embeds documented Julia bindings from the named modules:

````markdown
```@autodocs
Modules = [Eunoia]
Private = false
```
````

`Modules` is required and accepts a literal vector of distinct, optionally
qualified module names. Each name must identify one extracted Julia module.
Submodules must be listed explicitly. `Private` and `Public` accept literal
booleans and both default to `true`. `Private = false` selects exported bindings
and bindings marked with Julia's `public` keyword; `Public = false` selects
private bindings. These filters affect embedded documentation only. Entries
follow module order, then modules, constants, types, functions, and macros,
with names sorted within each category. Callable entries retain their methods
and constructor documentation. Fields remain addressable through the canonical
API surface.

An empty `@index` block lists the entries assembled by `@autodocs` in the same
collection and links to their page anchors. It may precede those blocks, and
it includes entries assembled on other pages. An item may appear in only one
`@autodocs` block per collection. Unsupported options, dynamic values,
duplicate options, and unknown modules produce source-attributed errors.
The supported options are `Modules`, `Private`, and `Public` for `@autodocs`;
`@index` takes no options.

Snapshots retain the assembled item references, original API documents,
resolved links, and local assets. Standalone HTML and HTML and Markdown
fragments render them without the checkout or Julia, including when the host
maps their URLs. This static profile uses `execution.mode: never`.
`@example`, `@repl`, and `jldoctest` fences display Julia code without execution;
other Documenter directives remain visible with unsupported-syntax diagnostics.

## Package landing pages {#package-landing-pages}

Every package has a landing page at `/packages/<slug>/`. A compact information
block above the content shows the package's available metadata and links. It
uses the same centered reading column as the page content, including on wide
screens. Missing metadata fields are omitted. Link icons distinguish source
repositories, issue trackers, websites, and supporting documents.

When a license page is available, the license value links to it. Diplodocus
renders conventional `LICENSE`, `LICENSE.md`, `LICENCE`, `LICENCE.md`,
`LICENSE-MIT`, `LICENSE-APACHE`, `COPYING`, and `COPYING.md` files at
`license.html` for every package. The snapshot retains their contents, and an
authored package page at `license.html` takes precedence.

To author the page, use a package-owned collection with an empty mount and an
`index.md` or `index.qmd` file. The page keeps its authored title
and the collection's normal execution settings. The package-information block
appears above the authored body.

Alternatively, set `overview` on the package declaration to select an existing
Markdown file:

```yaml
packages:
  - id: "python"
    name: "Models for Python"
    slug: "python"
    ecosystem: "python"
    repository: "core"
    path: "python"
    metadata-path: "pyproject.toml"
    overview: "docs/introduction.md"
    targets: []
```

The path is relative to the package root and must name a `.md` file inside the
package. Its code fences remain display-only. Links and images resolve relative
to that file. Diplodocus replaces an initial README-style heading with the
configured package display name, preserving its anchor and avoiding a second
top-level heading.
If a content collection contains the selected file, Diplodocus mounts that file
only at the package root rather than creating a second page for it.

An authored package index takes precedence over `overview`. Without either,
Diplodocus uses the package-root `README.md`, then the package description when
available. If neither exists, the page contains just the package information.
The selected content, metadata, and assets are retained in the snapshot, so
`generate` needs no checkout or language runtime.

R packages also have [automatically discovered supporting pages](extraction.md).
Their routes are `news.html`, `authors.html`, `license.html`, and
`community.html`; an authored page at one of these routes takes precedence.

## Navigation

The navigation bar links to the shared Guide, configured project sections, and
each visible package. Diplodocus
uses language labels such as Python, R, Julia, and Rust when there is one visible
package in that language; otherwise, it uses package names. The sidebar shows
only the current section's pages, plus its API reference for package sections.
On mobile, open **Menu** to browse documentation pages in a drawer. Use the
hamburger menu in the top navbar to switch sections.

Pages are grouped by their first URL directory within the section, including
the collection's mount. A group's `index.md` or `index.qmd` supplies its label
through its page title; otherwise, Diplodocus uses the directory name. For example,
mounting R content at `vignettes` creates a Vignettes group. Empty groups are
omitted. Shared concepts with authored documentation appear in the Guide sidebar.

## Supported writing surface

GFM pages support headings, paragraphs, emphasis, links, images, lists, quotes,
tables, thematic breaks, inline `$...$` math, display `$$...$$` math, and
display-only code fences. Equations render during the build with native KaTeX,
including HTML and MathML, bundled CSS, and local fonts. GFM pages need no
browser math renderer or R runtime. Escape literal dollar signs with a
backslash or put them in code spans. All three formats support YAML front
matter with a `layout` option. QMD pages add callouts and braced code cells.
The supported QMD document metadata includes `title`, `audience`, `layout`, and
execution settings. Supported cell options
include `eval`, `echo`, `include`, `output`, `error`, `label`, `fig-alt`,
`fig-cap`, and `fig-subcap`. Cell options may appear in a fence's info string or
in `#|` lines; the latter take precedence.

Add a language label such as `python`, `r`, `julia`, or `rust` to a code fence to enable
syntax highlighting. Diplodocus also highlights visible QMD cell source and API
signature blocks. Highlighting follows the site's light or dark theme and works without
JavaScript. Unlabeled fences, unknown languages, and `text` fences remain plain.
Plain-text execution output and errors also remain unhighlighted.

Diplodocus implements these selected profiles, not the full Quarto or Pandoc
languages. Unsupported constructs remain visible as placeholders and produce
diagnostics. Raw authored HTML and arbitrary directives are outside the supported surface.
R Markdown additionally supports math, footnotes, citations, simple tables, and
definition lists. Run `diplodocus check` after editing
a page to see parser and link diagnostics before any cell executes.

## Page layout

Add `layout: full` to a page's YAML front matter to hide the documentation
sidebar and the **On this page** table of contents and use the landing page's
wide, centered container:

```markdown
---
layout: full
---

# Project overview

Your page content goes here.
```

This works in `.md`, `.qmd`, and `.Rmd` pages. The navbar, search, and
configured footer remain available. The page keeps its title and content without
adding the homepage hero. Its links, search entry, and appearance in other
pages' sidebars are unchanged. Omit `layout` or set `layout: docs` for the
default documentation layout. At the site root, `presentation.home` takes
precedence.

## Tabsets

Use a fenced div with the `.panel-tabset` class to present alternatives in tabs.
The highest-level headings directly inside the div become tab labels. Lower-level
headings remain inside their panels. This syntax works in GFM, QMD, and R Markdown:

````markdown
::: {.panel-tabset group="language"}

### Python

```python
print("Hello")
```

### R {.active}

```r
print("Hello")
```

:::
````

The first tab opens by default; add `.active` to another tab heading to select it
instead. The optional `group` attribute synchronizes tabs with matching labels
in other tabsets in the same group on the page. Readers can switch tabs with the
arrow keys, Home, or End. Links into a hidden panel reveal it automatically.
Without JavaScript, all panels appear in document order under their headings.
Markdown exports retain every panel as ordinary sections.

## Links and assets

Use ordinary relative links between declared pages, including links across
collections. For example, `[Commands](../reference/cli.md)` targets the CLI
reference in another collection.
Local fragment links must name an existing heading anchor. Keep images and other
local assets inside a declared source boundary and link to them with relative
paths. Diplodocus validates local destinations and copies accepted assets into
the generated site. Rendered PNG and JPEG images include their intrinsic width
and height so browsers can reserve space before loading them. Diplodocus
preserves the original image bytes for downloads and fallback rendering.

During generation, Diplodocus also creates smaller PNG and JPEG variants for
large images and adds `srcset` and `sizes` to the HTML. Browsers can download a
variant that suits the display width and pixel density. Content variants have
widths of 480, 960, 1440, and 1920 pixels; navbar logos use 32, 64, and 96 pixels.
Diplodocus never enlarges images and only offers variants smaller in bytes than
the original. PNG variants retain transparency, and JPEG variants use
quality 90. Variants retain embedded ICC color profiles. SVGs, animated PNGs, and images
with an EXIF rotation or reflection remain unchanged.

Variants are generated from snapshot bytes, so generation does not need the
source checkout or another execution run. Original assets and Open Graph links
keep their existing URLs. Fragment output includes the variants and uses
`100vw` as a conservative size for the embedding site's unknown layout.
External HTTP links remain external.

Code fences in GFM and API documentation are always display-only. R Markdown chunks execute during extraction. QMD cells run
only when their collection explicitly authorizes execution; see [execution and
security](execution.md) before enabling them.

## R Markdown articles

Diplodocus discovers `.Rmd` and `.rmd` files under an R package's `vignettes/`
directory and mounts them beneath `vignettes`. To choose a different mount,
declare a package-owned collection with `format: "rmd"`. Explicit collections
replace automatic discovery for the same source files.

R Markdown uses native knitr execution with no kernel or execution opt-in.
Prepare R, pkgload, evaluate, knitr, package dependencies, and prebuilt native
extensions before building. Each article loads the supplied checkout in a fresh
R process. Chunks and inline R share state within that article. Relative data
paths resolve from the article's directory. `check` validates sources and links
without loading the package or evaluating R.

Fence options and `#|` options retain knitr semantics, including setup defaults,
computed `eval`, hidden chunks, `results = "asis"`, `knitr::kable()`, warnings,
messages, explicit permitted errors, and multiple captioned figures. Hidden
chunks still execute; `eval = FALSE` remains illustrative, and computed false
conditions retain skip outcomes. Unexpected errors stop publication. Local
support files participate in input validation and preview watching.

Console output appears separately from source code, without a shaded background,
syntax highlighting, or a copy button. This also applies to warnings, messages,
and permitted errors, including when `collapse = TRUE`. Source code keeps its
usual highlighting and copy button. `results = "asis"` retains its Markdown
formatting.

Title, author, and evaluated date metadata appear in the article header.
`bibliography` names one local BibTeX file or a list of files; optional `csl`
names a local independent CSL style. Without a style, citations use Chicago
author-date. Citations link to a formatted reference list beneath References.
The CSL style and locale archive comes from Hayagriva and the upstream
[Citation Style Language project](https://citationstyles.org/), with its
CC BY-SA 3.0 attribution retained in the archive.

Diplodocus renders the evaluated document itself. The `output` and `vignette`
metadata do not select an rmarkdown HTML template, and neither rmarkdown nor
Pandoc is required. This release supports R chunks, PNG figures, and static
Markdown content. Other chunk engines, child documents, persistent knitr caches,
interactive widgets, and README.Rmd rendering are unsupported. Runtime failures
leave the previous snapshot and site available. A completed snapshot can be
rendered with `generate` after removing the checkout and R.

## Browser components

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

Mount it in a GFM or QMD source, or an R Markdown collection with execution
disabled:

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
