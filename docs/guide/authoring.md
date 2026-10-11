# Authored pages

Declare each source directory as a `[[content]]` collection in [workspace
configuration](configuration.md). Set `format = "gfm"` for `.md` files or
`format = "qmd"` for `.qmd` files. A collection's `mount` determines its URL
prefix; an empty mount places its `index.md` at the site root. The `owner`
determines whether pages appear in project or package navigation.

## Navigation

The navigation bar links to the shared Guide and each visible package. Diplodocus
uses language labels such as Python, R, and Julia when there is one visible
package in that language; otherwise, it uses package names. The sidebar shows
only the current section's pages, plus its API reference for package sections.
On mobile, open **Browse documentation** to switch sections or browse their pages.

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

Diplodocus implements these selected profiles, not the full Quarto or Pandoc
languages. Unsupported constructs remain visible as placeholders and produce
diagnostics. Raw authored HTML, TeX math, footnotes, and arbitrary directives
are outside the current supported surface. Run `diplodocus check` after editing
a page to see parser and link diagnostics before any cell executes.

## Links and assets

Use ordinary relative links between declared pages, including links across
collections. For example, `[Commands](cli.md)` targets a page in this guide.
Local fragment links must name an existing heading anchor. Keep images and other
local assets inside a declared source boundary and link to them with relative
paths. Diplodocus validates local destinations and copies accepted assets into
the generated site. External HTTP links remain external.

Code fences in GFM and API documentation are always display-only. QMD cells run
only when their collection explicitly authorizes execution; see [execution and
security](execution.md) before enabling them.
