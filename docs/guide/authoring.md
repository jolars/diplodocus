# Authored pages

Declare each source directory as a `[[content]]` collection in
[workspace configuration](configuration.md). Set `format = "gfm"` for `.md`
files or `format = "qmd"` for `.qmd` files. A collection's `mount` determines
its URL prefix; an empty mount places its `index.md` at the site root. The
`owner` determines whether pages appear in project or package navigation.

## Supported writing surface

GFM pages support headings, paragraphs, emphasis, links, images, lists,
quotes, tables, thematic breaks, and display-only code fences. QMD pages add
YAML front matter, callouts, and braced code cells. The supported document
metadata includes `title`, `audience`, and execution settings. Supported cell
options include `eval`, `echo`, `include`, `output`, `error`, `label`, `fig-alt`,
`fig-cap`, and `fig-subcap`. Cell options may appear in a fence's info string
or in `#|` lines; the latter take precedence.

Diplodocus implements these selected profiles, not the full Quarto or Pandoc
languages. Unsupported constructs remain visible as placeholders and produce
diagnostics. Raw authored HTML, TeX math, footnotes, and arbitrary directives
are outside the current supported surface. Run `diplodocus check` after editing
a page to see parser and link diagnostics before any cell executes.

## Links and assets

Use ordinary relative links between declared pages, including links across
collections. For example, `[Commands](cli.md)` targets a page in this guide.
Local fragment links must name an existing heading anchor. Keep images and
other local assets inside a declared source boundary and link to them with
relative paths. Diplodocus validates local destinations and copies accepted
assets into the generated site. External HTTP links remain external.

Code fences in GFM and API documentation are always display-only. QMD cells
run only when their collection explicitly authorizes execution; see
[execution and security](execution.md) before enabling them.
