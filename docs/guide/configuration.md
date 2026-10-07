# Workspace configuration

A `diplodocus.toml` file declares the repositories, packages, and authored
content that belong in one site. Diplodocus does not discover nearby packages
or documents. Start from the working `examples/monorepo/diplodocus.toml`
configuration and adapt its paths and names. The root `diplodocus.toml`
shows an authored documentation site without API packages. See the
[quick start](quick-start.md) for commands to build either site.

## Repositories and packages

Each repository path is relative to the configuration directory; it may point
to a sibling checkout. A package names its repository and gives a path within
it. Package metadata and extraction targets are relative to the package. Declare
`targets = []` for a package with no API extraction.

Python extraction reads source and package metadata without importing code. R
extraction reads source, package metadata, and checked-in Rd files without
starting R. Julia extraction reads `Project.toml`, an explicit `.jl` entry
file, and its literal includes without starting Julia. The [extraction guide](extraction.md) describes their supported
inputs and limits.

## Authored content

Each `[[content]]` collection names a repository, source directory, mount,
format, and owner. The `project` owner puts pages in project navigation; a
package ID puts them under `/packages/<slug>/`. GFM collections read `.md` files.
QMD collections read `.qmd` files and can include executable cells. Checked-in
assets can live beside the pages that use them.

The monorepo example has one shared GFM guide and three statically extracted
packages. The root configuration mounts this GFM guide at the site root and
keeps its QMD example under `/examples/`. See [authored pages](authoring.md)
for supported syntax, links, and assets.

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

An optional table controls the site name, HTML description, and public URL:

```toml
[presentation]
title = "Foo documentation"
description = "Guides and API documentation for Foo."
site-url = "https://example.com/docs/"
canonical-url-style = "file"
```

The title defaults to `project.name`. Diplodocus uses its built-in theme.
Generated sites and portable snapshots keep the presentation metadata, so
`generate` does not need the original configuration. Read about
[portable snapshots](snapshots.md) before separating extraction and generation.

Set `site-url` to generate `sitemap.xml`, `robots.txt`, and canonical links in
HTML pages. It must be an absolute HTTP or HTTPS URL without credentials,
query parameters, fragments, or whitespace. Include the hosting prefix when
deploying under a subdirectory. Diplodocus treats the URL as a directory root,
whether or not you include its trailing slash. Omitting `site-url` omits all
three forms of crawler metadata.

The sitemap lists the homepage, authored pages, package overviews, API pages,
and concepts. It follows search visibility: public and internal package pages
appear, and hidden package pages do not. Assets do not appear. Entries contain
URLs only; Diplodocus does not infer modification dates from build times.

`canonical-url-style` defaults to `file`, which retains generated filenames
such as `/docs/index.html` and `/docs/installation.html`. Use `clean` when your
host serves index pages at directory URLs and other HTML pages without their
extension. For example, those URLs become `/docs/` and `/docs/installation`.
The Diplodocus documentation uses `clean` to match Cloudflare's default static
asset routing. This setting changes sitemap URLs and canonical links; output
filenames, navigation links, and local preview routes retain their file paths.
Clean URLs require matching support from your host. Ambiguous routes, such as
`foo.html` alongside `foo/index.html`, fail generation before site publication.

The generated `robots.txt` allows crawling and advertises the sitemap's full
URL. Crawlers read this file from the host's root `/robots.txt`. For a site
deployed under `/docs/`, add the generated `Sitemap` directive to the host's
root robots file. A file at `/docs/robots.txt` does not provide crawler discovery.
The first implementation writes one sitemap and rejects sites that exceed
the sitemap protocol's URL count, file size, or URL length limits.

Return to the [quick start](quick-start.md) or see the [commands](cli.md).
