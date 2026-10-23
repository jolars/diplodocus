# Authored pages

Declare each source directory as a collection in `content` in [workspace
configuration](configuration.md). Set `format: "gfm"` for `.md` files or
`format: "qmd"` for `.qmd` files, or `format: "rmd"` for R Markdown articles. A collection's `mount` determines its URL
prefix; an empty mount places its `index.md` at the site root. The `owner`
determines whether pages appear in project or package navigation.

R packages also have [automatically discovered package pages](extraction.md).
Use a package-owned collection with an empty mount and `index.md` or `index.qmd`
to replace the automatic overview. The metadata panel remains available. The
supporting routes are `news.html`, `authors.html`, `license.html`, and
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
tables, thematic breaks, and display-only code fences. QMD pages add YAML front
matter, callouts, and braced code cells. The supported document metadata
includes `title`, `audience`, and execution settings. Supported cell options
include `eval`, `echo`, `include`, `output`, `error`, `label`, `fig-alt`,
`fig-cap`, and `fig-subcap`. Cell options may appear in a fence's info string or
in `#|` lines; the latter take precedence.

Add a language label such as `python`, `r`, `julia`, or `rust` to a code fence to enable
syntax highlighting. Diplodocus also highlights visible QMD cell source and API
signature blocks. Highlighting uses a built-in light theme and works without
JavaScript. Unlabeled fences, unknown languages, and `text` fences remain plain.
Plain-text execution output and errors also remain unhighlighted.

Diplodocus implements these selected profiles, not the full Quarto or Pandoc
languages. Unsupported constructs remain visible as placeholders and produce
diagnostics. Raw authored HTML and arbitrary directives are outside the supported surface.
R Markdown additionally supports math, footnotes, citations, simple tables, and
definition lists. Run `diplodocus check` after editing
a page to see parser and link diagnostics before any cell executes.

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
the original. PNG variants retain transparency, and JPEG variants use quality
90. Variants retain embedded ICC color profiles. SVGs, animated PNGs, and images
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
