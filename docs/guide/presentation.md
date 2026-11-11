# Site appearance


An optional `presentation` mapping controls the site name, logo, link preview
image, HTML description, public URL, and footer:

```yaml
presentation:
  title: "Foo documentation"
  description: "Guides and API documentation for Foo."
  site-url: "https://example.com/docs/"
  canonical-url-style: "file"
  footer:
    docs-version: "v1.0.0"
    copyright: "© 2026 Foo contributors"
    licenses:
      - label: "MIT"
        url: "https://github.com/example/foo/blob/main/LICENSE"
    links:
      - label: "GitHub"
        url: "https://github.com/example/foo"
      - label: "Report an issue"
        url: "https://github.com/example/foo/issues"
    built-with: true
```

The title defaults to `project.name`. Diplodocus uses its built-in theme, which
follows the visitor's system color preference until they use the **Dark mode**
toggle in the header, or in the navigation menu on phones. The browser remembers
their light or dark choice across visits. With JavaScript disabled, the theme
follows the system preference.

## Page titles and descriptions {#page-titles-and-descriptions}

Use `presentation.pages` to override head metadata for individual pages,
including generated package overviews, license pages, and API references:

```yaml
presentation:
  title: Eunoia
  description: Area-proportional Euler and Venn diagrams.
  pages:
    index.html:
      title: Eunoia
    packages/julia/index.html:
      title: Eunoia for Julia · Eunoia
      description: Fit Euler diagrams with the Eunoia.jl package.
    packages/python/license.html:
      title: Python license · Eunoia
```

Keys name generated HTML paths relative to the site root, including `.html`.
They omit a leading slash and any hosting prefix, even with clean URLs. Use the
generated reference page's HTML path for an API override. Redirect sources,
fragments, and paths that do not name generated pages are rejected.

An explicit `title` supplies the complete browser tab and social card title.
Diplodocus does not append the site title to an override. Without an override,
the title uses the existing authored or generated page title followed by ` · `
and the site title. If both titles match, the title appears once. Title
overrides must not be empty or contain only whitespace. They do not change
headings, navigation labels, search titles, Markdown companions, or API names.
Duplicate HTML title diagnostics use the resolved titles.

A page `description` takes precedence over `presentation.description`. Omitting
it inherits the site description; if neither is set, description tags are
omitted. An empty string clears the description by emitting empty tags. Titles
and descriptions are plain text and are escaped for HTML. The resolved values
supply the HTML title and description, Open Graph, and Twitter cards. Social
cards still require `presentation.open-graph`.

Snapshots retain these declarations. `build`, detached `generate`, and native
`serve` resolve the same metadata. Preview rebuilds after configuration edits.
Fragment bundles leave head metadata to their frontend.

## Public assets and icons {#public-assets-and-icons}

Declare files that must appear at stable public paths, including files that no
authored page links to:

```yaml
presentation:
  assets:
    - repository: docs
      path: web/static/favicon.svg
      publish: favicon.svg
    - repository: docs
      path: web/static/favicon.ico
      publish: favicon.ico
    - repository: docs
      path: web/static/apple-touch-icon.png
      publish: apple-touch-icon.png
    - repository: docs
      path: web/static/site.webmanifest
      publish: site.webmanifest
    - repository: docs
      path: web/static/llms.txt
      publish: llms.txt
  icons:
    - path: favicon.svg
      type: image/svg+xml
      sizes: any
    - path: favicon.ico
    - path: apple-touch-icon.png
      rel: apple-touch-icon
    - path: site.webmanifest
      rel: manifest
```

Each source file must stay inside its declared repository. `publish` names a
file relative to the site root, without a leading slash or hosting prefix.
Declare individual files; directories and globs are not supported. Generated
pages, Markdown companions, clean URLs, redirects, and reserved output paths
cannot be replaced by public files. Public files may share a directory with
generated pages.

Icon paths refer to publication paths in `assets`. Diplodocus checks the link
relationship, file extension, optional MIME type, and optional sizes, then adds
escaped links to every complete HTML page. Links resolve correctly from nested
pages, clean URLs, and sites hosted below a prefix. Fragment bundles retain the
files and leave head links to their frontend.

Public files are explicit, trusted site inputs, like browser component bundles.
Diplodocus copies their bytes unchanged, including SVGs, manifests, and text.
Declare manifest icons and other dependent files separately. Relative URLs
inside those files resolve from their publication paths; root-relative URLs
still refer to the origin root. Diplodocus does not rewrite their contents.

Snapshots retain declarations, publication paths, and deduplicated file bytes.
`build`, detached `generate`, and native `serve` use the same files and icon
links. Preview watches declared source files outside content collections and
rebuilds after edits, removal, and recreation. A failed rebuild keeps serving
the preceding complete site.

## Landing page

Add `presentation.home` to give the site root a full-width landing page. It
shows the logo and title at the top left, beside the section links, search, and
repository links. Documentation pages keep the branding in the same position
at the top of the gray sidebar. Navigation keeps
the header and sidebar mounted while page content updates. Navbar links keep
the same positions when moving between the landing page and documentation.

```yaml
presentation:
  home:
    tagline: "Four languages.\nOne home for docs."
    description: "Guides and API references for all your packages."
    guide: "guide/index.html"
    actions:
      - label: "Get started"
        path: "guide/quick-start.html"
      - label: "Read the guide"
        path: "guide/index.html"
        icon: "book-open"
    features:
      - title: "Shared documentation"
        description: "Link across your packages."
        path: "guide/authoring.html"
```

Mount your guide collection at `guide` to keep it separate from the homepage.
`guide` sets the navbar's Guide destination. Action and feature paths name
visible generated HTML pages, including `.html`, even when the site uses clean
URLs. Diplodocus checks these destinations and renders links using the site's
URL settings. The first action receives the primary button style.

Actions accept an optional `icon`: `arrow-right`, `book-open`, `code`, `download`,
or `terminal`. The built-in SVG appears before the label and inherits the
button's text color. Icons are decorative, so the label still supplies the
accessible link name. Omitting `icon` displays only the label. Unknown icon
names are rejected.

The hero uses the site title and optional logo. All text is plain text, and
newlines in `tagline` separate heading lines. Actions and features can be
omitted. If an authored page already occupies the site root, its body appears
below the landing content, with its initial title supplied by the hero.
Without `presentation.home`, the root retains the documentation layout.

## Branding

To add a logo before the title in the leftmost navbar link, declare an image in
one of your repositories:

```yaml
presentation:
  logo:
    repository: "docs"
    path: "assets/logo.svg"
```

`repository` must match a declared `repositories` ID. `path` is relative to
that repository's root and must name an SVG, PNG, or JPEG file. The image can
live outside a content collection. Use forward slashes without leading slashes
or `.` or `..` components. Symlinks must stay inside the repository.

Diplodocus validates and embeds the image in the snapshot, so generation needs
no source checkout. The logo keeps its proportions within the navbar and shares
the title's link to the homepage. The title supplies the accessible link name.
Omitting `presentation.logo` leaves the navbar with the title alone. `serve`
watches the logo file and rebuilds when it changes.

To use a shared image when people post links to your documentation, set
`site-url` and declare an Open Graph image:

```yaml
presentation:
  open-graph:
    repository: "docs"
    path: "assets/og.png"
    alt: "Foo documentation, with the project mascot beside an open book."
```

The image must be a PNG or JPEG inside the declared repository. `alt` describes
what the image contains and must not be empty. A 1200 by 630 pixel image works
well for a wide preview. The image can live outside content collections and
follows the same path and symlink rules as the navbar logo.

Every generated HTML page receives Open Graph metadata and a large-image
Twitter card. The preview uses that page's resolved title, description, and
canonical URL, the site name, and the shared image with its dimensions and alt
text.
Image URLs are absolute and include any hosting prefix from `site-url`.
Diplodocus validates and stores the image in the snapshot, so `generate` needs
no source checkout. `serve` watches the image for changes. Omitting the mapping
omits social preview metadata.

The footer appears at the bottom of every page. All its fields are optional,
and omitting the mapping produces no footer. `docs-version` is a free-form label
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

## Redirects after moving pages {#redirects-after-moving-pages}

Declare old URLs in `presentation.redirects` when moving documentation:

```yaml
presentation:
  site-url: "https://example.org/docs/"
  canonical-url-style: "clean"
  redirects:
    /installation: "guide/installation.html"
    /installation.html: "guide/installation.html"
    /old-guide/: "guide/index.html"
```

Each key is an exact old URL path beginning with `/`, relative to the site's
root. Omit the hosting prefix. Each value names a visible generated HTML page,
including its `.html` extension, as navigation destinations do. Diplodocus
resolves the destination using `canonical-url-style`. The example redirects
`/docs/installation` and `/docs/installation.html` to `/docs/guide/installation`
with HTTP 301 responses.

Declare each old spelling you need to preserve. A trailing slash is significant.
Sources may contain Unicode or percent-encoded characters; encode spaces as
`%20`. Existing escapes keep their exact spelling and case to match static hosts.
Paths that decode to the same source cannot be declared twice. Encoded separators,
control characters, and dot segments are rejected. Wildcards, placeholders,
external destinations, query-specific rules, and
fragments are not supported. Targets must exist, and sources cannot replace
current pages, their Markdown exports, or reserved output paths. These checks
also prevent redirect chains and loops. `diplodocus check` validates
declarations without publishing output.

The standalone generator writes a deterministic `_redirects` file for
[Cloudflare static
assets](https://developers.cloudflare.com/workers/static-assets/redirects/) and
[Netlify](https://docs.netlify.com/manage/routing/redirects/overview/). It
limits the file to 2,000 rules and each rule to 1,000 characters, matching
Cloudflare's static redirect limits. Other hosts need equivalent server rules;
the generated file does not itself make those hosts redirect. When deploying
under a prefix, place the rules in the host's redirect configuration at its
publish root.

`diplodocus serve` returns the same permanent redirects locally and preserves
query strings. Preview destinations use its local root, without the public
hosting prefix. Redirects are retained in snapshots, so `generate` recreates the
rules without source files. They do not create duplicate pages or entries in
search or the sitemap. Fragment generation leaves redirects to the external
frontend and does not write `_redirects`.

## Preserving old fragments {#preserving-old-fragments}

Declare fragment aliases when a heading or API anchor changes. Attach each old
ID to its maintained heading or canonical anchor:

```yaml
presentation:
  redirects:
    /old-guide.html: guide/start.html
  fragment-aliases:
    guide/start.html:
      Getting-Started: { heading: Getting started }
      autotoc_md0: { anchor: installation }
```

With this example, `/old-guide.html#Getting-Started` redirects to the current
page and reaches its Getting started heading. The current heading keeps its
canonical ID. Several aliases can point to the same heading, including a title
heading without a canonical ID.

Use exact plain-text heading labels, omitting Markdown formatting. Whitespace is
collapsed when matching. If a page repeats a heading, select its unique
canonical ID with `anchor`. API member and section IDs appear in generated HTML
and the fragment bundle's `documents.json`. Aliases on embedded fields and
methods belong to the HTML page that contains those targets.

Write literal IDs in configuration, including any spaces or punctuation in old
Julia signatures. Percent-encode those characters in links. Diplodocus rejects
missing targets and conflicting IDs instead of silently losing old links. TOCs
and search continue to use canonical links.

Aliases remain in HTML, Markdown exports, portable snapshots, and native
preview. A relocated snapshot recreates them without the source checkout. Native
preview rebuilds after configuration edits and keeps the preceding complete site
if an alias becomes invalid.

## Crawler discovery

The generated `robots.txt` allows crawling and advertises the sitemap's full
URL. Crawlers read this file from the host's root `/robots.txt`. For a site
deployed under `/docs/`, add the generated `Sitemap` directive to the host's
root robots file. A file at `/docs/robots.txt` does not provide crawler
discovery. The first implementation writes one sitemap and rejects sites that
exceed the sitemap protocol's URL count, file size, or URL length limits.


See [navigation](navigation.md) to arrange your pages and
[the configuration reference](../reference/configuration.md#presentation) for
all presentation fields and defaults.
