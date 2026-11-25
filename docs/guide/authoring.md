# Writing pages

Declare shared pages under [`content`](configuration.md#add-pages) and
package pages under `packages[].content`. A collection selects a source
directory or file, and its `mount` sets the URL prefix. For
example, a project-owned `CHANGELOG.md` with `mount: project` becomes
`/project/CHANGELOG.html`. Links and images resolve from the source directory.

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

The page appears at `guide/tutorial.html`. Run `diplodocus check` to catch
broken links, then use `diplodocus serve` to preview edits.

## Choose a format {#choose-a-format}

| `format` | Source | Use it for |
| --- | --- | --- |
| `gfm` | `.md` | Markdown guides with display-only code fences. |
| `qmd` | `.qmd` | Quarto Markdown callouts and optionally executable code cells. |
| `rmd` | `.Rmd`, `.rmd` | [R Markdown articles](extraction/r.md#r-markdown-articles) evaluated with knitr. |
| `documenter` | `.md` | [Julia Documenter pages](extraction/julia.md#documenter-pages) with API links and directives. |

### QMD cells

QMD cells use braces around the language, such as `{python}` on the opening
fence. They display code by default. To run them, install the Jupyter kernel
you need and set the collection's
[`execution`](../reference/configuration.md#collection-execution) to
`mode: execute`, `engine: jupyter`, and a `kernel` such as `python3`.
Cells on a page share one kernel session.

Set cell options in the fence's info string or in `#|` lines inside the fence.
The `#|` lines take precedence. Use `eval: false` to skip a cell or
`echo: false` to hide its source. Other supported options are `include`,
`output`, `error`, `label`, `fig-alt`, `fig-cap`, and `fig-subcap`.

`diplodocus check` validates pages and links without running code cells.
During extraction, enabled cells run with the permissions of the user or CI
job building the site. Review the source before building a workspace that
executes code.

## Package landing pages {#package-landing-pages}

Every package has a landing page at `/packages/<slug>/`. It shows available
package metadata and links above the content.

When a license page is available, the license value links to it. Diplodocus
renders conventional `LICENSE`, `LICENSE.md`, `LICENCE`, `LICENCE.md`,
`LICENSE-MIT`, `LICENSE-APACHE`, `COPYING`, and `COPYING.md` files at
`license.html` for every package. The snapshot retains their contents, and an
authored package page at `license.html` takes precedence.

To author the page, add a collection under the package with an empty mount and an
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
complete example.

## Supported writing surface

Use Markdown headings, links, images, lists, quotes, and tables. Write inline
math with `$...$` and display math with `$$...$$`. Escape literal dollar signs
with a backslash or put them in code spans.

Add a language label such as `python` to a code fence for syntax highlighting.
Use `text` for plain output. Run `diplodocus check` to see diagnostics for
unsupported markup, including raw HTML and unrecognized directives.

## Page layout

Choose a page layout in YAML front matter for GFM, QMD, or R Markdown:

  | Layout | Width and navigation                                                                     |
  | ------ | ---------------------------------------------------------------------------------------- |
  | `docs` | Default documentation layout, with a sidebar and the **On this page** table of contents. |
  | `wide` | The landing page's wide, centered container, without either sidebar.                     |
  | `full` | The navbar's full container and matching outer gutters, without either sidebar.          |

For example, use `wide` for a project overview:

```markdown
---
layout: wide
---

# Project overview

Your page content goes here.
```

Use `layout: full` when tables or plots need the same width as the navbar.
Both wider layouts retain the navbar, search, and footer. At the site root,
`presentation.home` takes precedence over the page layout.

## Tabsets

Use a fenced div with the `.panel-tabset` class to present alternatives in tabs.
The highest-level headings directly inside the div become tab labels. Lower-level
headings remain inside their panels. This syntax works in GFM, QMD, and R Markdown:

````markdown
::: {.panel-tabset group="language"}

### Python {icon="python"}

```python
print("Hello")
```

### R {.active icon="r"}

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

The optional `icon` attribute adds a decorative icon beside a tab label.
Use `cpp`, `julia`, `python`, `r`, or `rust` for languages, and `linux`,
`macos`, or `windows` for platforms. Keep the label in the heading, for
example `### macOS {icon="macos"}`.

## Links and assets

Use ordinary relative links between declared pages, including links across
collections. For example, `[CLI Reference](../reference/cli.md)` targets the CLI
reference in another collection.
Local fragment links must name an existing heading anchor. Keep images and other
local assets inside a declared source boundary and link to them with relative
paths. Diplodocus validates local destinations and copies accepted assets into
the generated site. It creates smaller variants of large PNG and JPEG images
and keeps the originals available. Readers' browsers load remote HTTP and
HTTPS images from their original hosts.

### SVG conversion

Local SVGs must use supported static markup. If Diplodocus reports
`invalid-content-image` for a plotting tool's SVG, export the figure as a PNG
or convert it with the repository's
[`convert-svg.py`](../../scripts/convert-svg.py) helper. With Cairo and `uv`
installed, run this command from a Diplodocus checkout:

```sh
uv run scripts/convert-svg.py /path/to/docs/images/plot.svg
```

The helper writes `plot.png` beside the SVG. Update the Markdown link to the
PNG, keep meaningful alternative text, and run `diplodocus check`.

## Include shared content

GFM and QMD pages can include whole local files, including nested partials.
For GFM, put this directive on its own unindented line, separated from other
content by blank lines:

```markdown
<!--@include: ./parts/basics.md-->
```

QMD uses Quarto's include syntax:

```markdown
{{< include ./parts/basics.qmd >}}
```

Paths resolve from the file containing the directive. Links, images, and nested
includes inside a partial resolve from that partial's directory. Use `@/` in
GFM or `/` in QMD for paths relative to the collection's source root. Targets
must stay inside the declared repository. GFM accepts `.md`; QMD accepts
`.md` and `.qmd`. Double-quote paths containing spaces.

Partials cannot declare frontmatter. The main page owns metadata, execution
settings, and the working directory for included cells. Including a file does
not change whether it also appears as a standalone page; keep partials outside
the collection directory when they should have no route. Missing files and
include cycles fail validation, with diagnostics pointing to the original
source and include chain.

Whole-file Markdown includes currently have no line, region, or heading
selectors. Includes apply only to GFM and QMD collections.

### Include source code

Place a code include on its own line inside a top-level fence. This GFM
example includes lines 3 through 8 of `demo.py`.

> ````markdown
> ```python
> <!--@include-code: ../examples/demo.py{3-8}-->
> ```
> ````

QMD uses shortcode syntax.

````markdown
```python
{{< include-code "../examples/my demo.py"{3-8} >}}
```
````

Omit `{3-8}` to include the entire file. Ranges are one-based and inclusive.
Code includes accept local UTF-8 files of any extension and use the same path
roots and repository boundary as Markdown includes. Each selected line keeps
its original indentation and receives any leading whitespace from the
directive line. Markdown syntax, fence markers, and nested directives in the
included lines stay inside the fence. In executable QMD cells, put cell options
in the page. A blank line separates options from included code that starts
with an option-like comment.

`include-code` is a Diplodocus extension. Whole-file Markdown directives remain
literal inside fences. Code directives inside indented code, block quotes,
and lists also remain literal.

## Browser components

For custom interactive tools, see [browser components](authoring/components.md)
in the Advanced section. Ordinary guides can use Markdown, code examples, and
images without a browser bundle.
