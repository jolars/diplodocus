# Writing pages

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

## Write a page

Start with a heading and ordinary Markdown. For a GFM collection whose path is
`docs` and mount is `guide`, save this as `docs/tutorial.md`:

````markdown
# Tutorial

Explain the task, then show a small example:

```python
values = [1, 2, 3]
print(sum(values))
```

Return to the [overview](index.md).
````

The page appears at `guide/tutorial.html`. Code fences display source without
running it. Run `diplodocus check` to catch broken links, then use
`diplodocus serve --live-reload` to preview edits. See
[executable examples](execution.md) when you want evaluated code and results.

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

R packages also have [automatically discovered supporting pages](extraction/r.md#package-landing-pages).
Their routes are `news.html`, `authors.html`, `license.html`, and
`community.html`; an authored page at one of these routes takes precedence.

## Navigation

Page headings provide titles, and directories provide automatic sidebar groups.
To choose your own order, labels, and nested groups, set
`presentation.sidebar`. Follow the [navigation guide](navigation.md) for a
complete example. The sidebar can differ between sections such as Guide and
Reference without changing your source layout.

## Supported writing surface

GFM pages support headings, paragraphs, emphasis, links, images, lists, quotes,
tables, thematic breaks, inline `$...$` math, display `$$...$$` math, and
display-only code fences. Equations render during the build with native KaTeX,
including HTML and MathML, bundled CSS, and local fonts. GFM pages need no
browser math renderer or R runtime. Escape literal dollar signs with a
backslash or put them in code spans. GFM, QMD, and R Markdown support YAML front
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
collections. For example, `[CLI Reference](../reference/cli.md)` targets the CLI
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

### SVG conversion

Local SVG images must satisfy the inert SVG allowlist. Diplodocus rejects DTDs,
style sheets and attributes, definitions, references, clipping paths, scripts,
and external resources. A rejected image produces `invalid-content-image` at the
reference's source location, including the asset path. A missing file still
produces `unresolved-document-reference`.

For static Matplotlib figures, convert the SVG to PNG before linking it. The
repository provides [`scripts/convert-svg.py`](../../scripts/convert-svg.py),
using CairoSVG 2.8.2 with an explicit resource fetcher that rejects every
secondary resource request. It retains local definitions, glyph references,
styles, clipping, and transparency. It ignores the unused Matplotlib DTD without
fetching it, rejects XML entities, active elements, and event handlers, and
removes metadata and XML processing instructions. Conversion preserves the
drawing at a fixed pixel density; it does not retain vector scaling or SVG
accessibility metadata. Keep meaningful alternative text on the Markdown image.

Install Cairo's shared library and `uv` first. From the Diplodocus checkout,
convert figures in a disposable copy of the source documentation:

```sh
trial=$(mktemp -d)
cp -R ../qualpal/docs "$trial/docs"
uv run scripts/convert-svg.py "$trial"/docs/images/examples/*.svg
```

On NixOS, expose Cairo's library to the Python process:

```sh
export LD_LIBRARY_PATH="$(nix build --no-link --print-out-paths nixpkgs#cairo)/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
```

Run that export before conversion. `uv` installs the Python dependencies
declared in the script. The converter writes a sibling `.png` for each `.svg`,
refuses to overwrite existing PNGs, and reports failures per input with a
nonzero exit status. The default `--scale 2` renders twice the SVG's nominal
pixel dimensions; increase it up to 8 for larger displays. Inspect the converted
figures before publishing them.

In the disposable guide files, change image destinations such as
`images/examples/hsl_pal.svg` to `images/examples/hsl_pal.png`. Convert each
distinct image once, including images referenced by multiple guides. Keep the
PNGs inside the declared content boundary, run `diplodocus check --config PATH`,
and then extract and generate normally. Snapshots retain the PNG bytes, so
detached generation needs neither the original SVGs nor CairoSVG. Conversion is
an explicit preparation step; Diplodocus does not invoke the converter during
extraction or generation.

Run the converter's regression tests with `uv run scripts/convert-svg.test.py`
in an environment with Cairo available. The resource boundary uses CairoSVG's
[renderer fetch
hook](https://github.com/Kozea/CairoSVG/blob/2.8.2/cairosvg/surface.py) and [XML
parser
protections](https://github.com/Kozea/CairoSVG/blob/2.8.2/cairosvg/parser.py).

Authored images can also use HTTP or HTTPS URLs, including linked README
badges. Diplodocus preserves these URLs in HTML and Markdown exports without
downloading, validating, or copying the remote image bytes. Browsers load those
images from their original hosts, so their contents can change and they require
a network connection. Remote images do not receive local image dimensions or
responsive variants. Generated execution output still requires validated local
image assets.

Code fences in GFM and API documentation are always display-only. R Markdown chunks execute during extraction. QMD cells run
only when their collection explicitly authorizes execution; see [executable examples](execution.md) before enabling them.


## R Markdown articles

Use R Markdown for articles that evaluate R chunks and inline expressions with
knitr. Diplodocus discovers package vignettes automatically, and you can also
declare an `rmd` collection. The [R Markdown guide](authoring/rmarkdown.md)
describes the environment, supported syntax, citations, and results.

## Documenter pages {#documenter-pages}

Use the Documenter format for Julia Markdown pages with `@ref` links, `@docs`
and `@autodocs` API assembly, and optional native `@example` execution. The
[Julia Documenter guide](authoring/documenter.md) describes each directive and
shows how to select a prepared Julia environment.

## Browser components

For custom interactive tools, see [browser components](authoring/components.md)
in the Advanced section. Ordinary guides can use Markdown, code examples, and
images without a browser bundle.
