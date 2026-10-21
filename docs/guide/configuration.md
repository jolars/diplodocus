# Workspace configuration

A `diplodocus.toml` file declares the repositories, packages, and authored
content that belong in one site. Diplodocus does not discover nearby packages or
documents. Start from the working `examples/monorepo/diplodocus.toml`
configuration and adapt its paths and names. The root `diplodocus.toml` shows an
authored documentation site without API packages. See the [quick
start](quick-start.md) for commands to build either site.

## Repositories and packages

Each repository path is relative to the configuration directory; it may point to
a sibling checkout. A package names its repository and gives a path within it.
Package metadata and extraction targets are relative to the package. Declare
`targets = []` for a package with no API extraction.

Python extraction reads source and package metadata. R reads source, package
metadata, and checked-in Rd files. Julia reads `Project.toml`, an explicit `.jl`
entry file, and its literal includes, with native completion when needed. Rust
reads Cargo metadata and compiler-produced rustdoc JSON for an explicit library
entry file. Native runtimes verify executable API examples. The [extraction
guide](extraction.md) describes the supported inputs and limits.

### Repository links

Automatically discovered package pages use repository links for existing local
targets that are not site pages or accepted downloads. For GitHub, set the
repository's `url` to `https://github.com/OWNER/REPO`. Diplodocus constructs
`blob` links for files and `tree` links for directories, using the declared
`revision` or the observed Git commit. It does not guess a default branch.
An optional `.git` suffix and trailing slash are accepted.

For other hosts, supply `source-link-template`. The template must contain
`{revision}` and `{path}`. Diplodocus encodes the revision as one URL segment
and encodes each component of the canonical repository-relative path. An
optional `{kind}` expands to `blob` for files or `tree` for directories;
directory links require it in a custom template. Unknown placeholders are
errors when the fallback is needed. Templates with `{line}` use `1`, but
the template's fragment is replaced with the authored fragment or removed
for a whole-file link. The resulting URL must use HTTP or HTTPS without
credentials. For example:

```toml
[[repository]]
id = "core"
path = ".."
revision = "v1.2.0"
source-link-template = "https://forge.example/team/core/{kind}/{revision}/{path}"
```

Repository links retain their canonical target and repository identity in the
snapshot. Generation derives the URL from that snapshot without accessing the
checkout or fetching the destination.

## Authored content

Each `[[content]]` collection names a repository, source directory, mount,
format, and owner. The `project` owner puts pages in project navigation; a
package ID puts them under `/packages/<slug>/`. GFM collections read `.md`
files. QMD collections read `.qmd` files and can include executable cells.
Checked-in assets can live beside the pages that use them.

The monorepo example has one shared GFM guide and four extracted packages,
including a compiler-backed Rust library. The root configuration mounts this GFM
guide at `/guide/` and
keeps its QMD example under `/examples/`. See [authored pages](authoring.md) for
supported syntax, links, and assets.

To give a project collection its own navbar tab and sidebar, declare a section:

```toml
[[presentation.sections]]
label = "Reference"
path = "reference/index.html"
```

The path names a generated, visible project `index.html` page. All project pages
under that directory belong to the section and leave the Guide sidebar.
Section directories must be distinct and cannot contain the Guide landing page.
Tabs appear after Guide in declaration order, before package tabs.

## Execution

Execution defaults to `never`. To allow it, set the collection to QMD and
declare a Jupyter engine and kernel explicitly:

```toml
[[content]]
id = "examples"
owner = "project"
repository = "diplodocus"
path = "docs/examples"
mount = "examples"
format = "qmd"

[content.execution]
mode = "execute"
engine = "jupyter"
kernel = "python3"
declared-environment-inputs = ["devenv.lock"]
```

Declared environment inputs are individual files relative to the repository.
They contribute to provenance and execution cache keys; Diplodocus does not
install or interpret them. Document metadata can restrict execution but cannot
enable it when the collection forbids it. `check` never runs cells. `extract`,
`build`, and `serve` may run authorized cells with your user permissions.

## Presentation

An optional table controls the site name, logo, link preview image, HTML
description, public URL, and footer:

```toml
[presentation]
title = "Foo documentation"
description = "Guides and API documentation for Foo."
site-url = "https://example.com/docs/"
canonical-url-style = "file"

[presentation.footer]
docs-version = "v1.0.0"
copyright = "© 2026 Foo contributors"
licenses = [
  { label = "MIT", url = "https://github.com/example/foo/blob/main/LICENSE" },
]
links = [
  { label = "GitHub", url = "https://github.com/example/foo" },
  { label = "Report an issue", url = "https://github.com/example/foo/issues" },
]
built-with = true
```

The title defaults to `project.name`. Diplodocus uses its built-in theme.
Generated sites and portable snapshots keep the presentation metadata, so
`generate` does not need the original configuration. Read about [portable
snapshots](snapshots.md) before separating extraction and generation.

### Landing page

Add `[presentation.home]` to give the site root a full-width landing page. It
shows the logo and title at the top left, beside the section links, search, and
repository links. Documentation pages keep the branding in the same position
at the top of the gray sidebar. Navigation keeps
the header and sidebar mounted while page content updates. Navbar links keep
the same positions when moving between the landing page and documentation.

```toml
[presentation.home]
tagline = "Four languages.\nOne home for docs."
description = "Guides and API references for all your packages."
guide = "guide/index.html"
actions = [
  { label = "Get started", path = "guide/quick-start.html" },
  { label = "Read the guide", path = "guide/index.html" },
]
features = [
  { title = "Shared documentation", description = "Link across your packages.", path = "guide/authoring.html" },
]
```

Mount your guide collection at `guide` to keep it separate from the homepage.
`guide` sets the navbar's Guide destination. Action and feature paths name
visible generated HTML pages, including `.html`, even when the site uses clean
URLs. Diplodocus checks these destinations and renders links using the site's
URL settings. The first action receives the primary button style.

The hero uses the site title and optional logo. All text is plain text, and
newlines in `tagline` separate heading lines. Actions and features can be
omitted. If an authored page already occupies the site root, its body appears
below the landing content, with its initial title supplied by the hero.
Without `[presentation.home]`, the root retains the documentation layout.

### Branding

To add a logo before the title in the leftmost navbar link, declare an image in
one of your repositories:

```toml
[presentation.logo]
repository = "docs"
path = "assets/logo.svg"
```

`repository` must match a declared `[[repository]]` ID. `path` is relative to
that repository's root and must name an SVG, PNG, or JPEG file. The image can
live outside a content collection. Use forward slashes without leading slashes
or `.` or `..` components. Symlinks must stay inside the repository.

Diplodocus validates and embeds the image in the snapshot, so generation needs
no source checkout. The logo keeps its proportions within the navbar and shares
the title's link to the homepage. The title supplies the accessible link name.
Omitting `[presentation.logo]` leaves the navbar with the title alone. `serve`
watches the logo file and rebuilds when it changes.

To use a shared image when people post links to your documentation, set
`site-url` and declare an Open Graph image:

```toml
[presentation.open-graph]
repository = "docs"
path = "assets/og.png"
alt = "Foo documentation, with the project mascot beside an open book."
```

The image must be a PNG or JPEG inside the declared repository. `alt` describes
what the image contains and must not be empty. A 1200 by 630 pixel image works
well for a wide preview. The image can live outside content collections and
follows the same path and symlink rules as the navbar logo.

Every generated HTML page receives Open Graph metadata and a large-image
Twitter card. The preview uses that page's title and canonical URL, the site
name and description, and the shared image with its dimensions and alt text.
Image URLs are absolute and include any hosting prefix from `site-url`.
Diplodocus validates and stores the image in the snapshot, so `generate` needs
no source checkout. `serve` watches the image for changes. Omitting the table
omits social preview metadata.

The footer appears at the bottom of every page. All its fields are optional,
and omitting the table produces no footer. `docs-version` is a free-form label
such as `v1.0.0`, `dev`, or `main`; it identifies the documentation release or
branch independently of individual package versions. Diplodocus does not infer
the label or copyright notice from package metadata or the build date.

`copyright` supplies the complete notice, including the symbol, years, and
holder. `licenses` links to alternative licenses for the site's content and
joins them with "or". Package-specific licenses remain on their package pages.
`links` supplies project links in display order. Each license or project link
requires a text `label` and an absolute HTTP or HTTPS `url` without credentials
or whitespace. Footer text is escaped rather than parsed as Markdown or HTML.
`built-with` defaults to `false`; setting it to `true` adds a linked
"Built with Diplodocus" credit.

Set `site-url` to generate `sitemap.xml`, `robots.txt`, and canonical links in
HTML pages. It must be an absolute HTTP or HTTPS URL without credentials, query
parameters, fragments, or whitespace. Include the hosting prefix when deploying
under a subdirectory. Diplodocus treats the URL as a directory root, whether or
not you include its trailing slash. Omitting `site-url` omits all three forms of
crawler metadata.

The sitemap lists the homepage, authored pages, package overviews, API pages,
and concepts. It follows search visibility: public and internal package pages
appear, and hidden package pages do not. Assets do not appear. Entries contain
URLs only; Diplodocus does not infer modification dates from build times.

`canonical-url-style` defaults to `file`, which retains generated filenames such
as `/docs/index.html` and `/docs/installation.html`. Use `clean` when your host
serves index pages at directory URLs and other HTML pages without their
extension. For example, those URLs become `/docs/` and `/docs/installation`. The
Diplodocus documentation uses `clean` to match Cloudflare's default static asset
routing. This setting changes page links, search results, sitemap URLs,
canonical links, and Open Graph page URLs. Output filenames retain their file
paths, and the local preview serves both clean URLs and generated file paths.
Clean URLs require matching support from your host. Ambiguous routes, such as
`foo.html` alongside `foo/index.html`, fail generation before site publication.

The generated `robots.txt` allows crawling and advertises the sitemap's full
URL. Crawlers read this file from the host's root `/robots.txt`. For a site
deployed under `/docs/`, add the generated `Sitemap` directive to the host's
root robots file. A file at `/docs/robots.txt` does not provide crawler
discovery. The first implementation writes one sitemap and rejects sites that
exceed the sitemap protocol's URL count, file size, or URL length limits.

Return to the [quick start](quick-start.md) or see the [commands](../reference/cli.md).
