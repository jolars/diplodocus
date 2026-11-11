# Configuration reference

This page lists every option in `diplodocus.yaml`. For a smaller example and an
explanation of the main choices, use the [configuration
guide](../guide/configuration.md). Command-line flags and output paths are in
the [CLI Reference](cli.md).

Field names use kebab-case. **Required** means that a field must appear when
its containing mapping or list entry is declared. Omitted optional lists are
empty. Unknown fields, duplicate keys, multiple YAML documents, and incorrect
field types are errors. Use YAML booleans such as `true`, rather than quoted
strings, for boolean options.

## Root fields

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `project` | mapping | Required | Project identity. |
| `presentation` | mapping | Built-in presentation | Site metadata, navigation, branding, and footer. |
| `repositories` | list | `[]` | Local source checkouts. |
| `packages` | list | `[]` | Packages or components to document. |
| `content` | list | `[]` | Authored page collections. |
| `components` | list | `[]` | Prebuilt browser bundles used by authored pages. |
| `concepts` | list | `[]` | Groups of corresponding API items. |
| `relationships` | list | `[]` | Implementation and dependency relationships between packages. |

## `project`

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `name` | string | Required | Project display name and default site title. |

## `repositories`

Each entry declares one local checkout. Repository IDs must be unique.

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `id` | string | Required | Identifier used by packages, collections, and images. |
| `path` | path | Required | Checkout root, resolved from the configuration directory. May name a sibling checkout. |
| `url` | string | Unset | Canonical repository URL. GitHub URLs support automatic source links. |
| `source-link-template` | string | Unset | Custom URL template for source files and directories. |
| `revision` | string | Observed Git commit, when available | Explicit branch, tag, or commit for source links and provenance. |

### Repository links {#repository-links}

For GitHub, use `https://github.com/OWNER/REPO` as `url`. An optional `.git`
suffix and trailing slash are accepted. Source links use `blob` for files and
`tree` for directories, with the declared revision or observed commit.
Diplodocus does not guess a default branch.

For other hosts, set `source-link-template`. It requires `{revision}` and
`{path}` placeholders. The optional `{kind}` expands to `blob` or `tree` and
is required for directory links. The revision is encoded as one URL segment,
and each path component is encoded separately. Unknown placeholders are errors
when the template is used. `{line}` uses `1`; a template fragment is replaced
with the authored fragment or removed for a whole-file link. The resulting URL
must use HTTP or HTTPS without credentials.

```yaml
repositories:
  - id: core
    path: ..
    revision: v1.2.0
    source-link-template: "https://forge.example/team/core/{kind}/{revision}/{path}"
```

Automatically discovered package pages can use these links for existing local
files and directories outside the published pages. See
[R package pages](../guide/extraction/r.md#package-landing-pages) for the
resolution rules.

## `packages`

Each entry declares a package or internal component. Package IDs and site-wide
slugs must be unique. Paths stay inside the declared repository and package
boundaries.

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `id` | string | Required | Stable package identifier used by references and owners. |
| `name` | string | Required | Package display name. |
| `slug` | string | Required | URL segment beneath `packages/`. |
| `ecosystem` | string | Required | Package language: `python`, `r`, `julia`, `rust`, or `cpp`. |
| `repository` | string | Required | Declared repository ID. |
| `path` | path | Required | Package root, relative to its repository. |
| `metadata-path` | path | Required | Metadata file, relative to the package root. |
| `overview` | path | `README.md`, then package description | Optional `.md` file relative to the package root. An authored package index takes precedence. |
| `kind` | string | `package` | `package` for a distribution or `component` for an internal component. |
| `visibility` | string | `public` | Navigation and search visibility, described below. |
| `targets` | list | Required | Explicit extraction targets. Use `[]` for no API extraction. |

`visibility` accepts:

- `public`: included in navigation and search.
- `internal`: addressable and searchable, but omitted from project navigation.
- `hidden`: available as a semantic target without navigation or search entries.

The conventional metadata files are `pyproject.toml`, `DESCRIPTION`,
`Project.toml`, `Cargo.toml`, and `Doxyfile`. See [API
extraction](../guide/extraction.md) for language-specific setup and [package landing
pages](../guide/authoring.md#package-landing-pages) for overview precedence.

### Extraction targets

Each `packages[].targets` entry selects one extractor and its source. Target
IDs must be unique within their package.

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `id` | string | Required | Target identifier within the package. |
| `extractor` | string | Required | `python`, `r`, `julia`, `rust`, or `doxygen`. |
| `path` | path | Required | Source path relative to the package root. |
| `role` | string | Required | Target role, such as `public-api` or `internal-api`. |
| `rust` | mapping | Default Cargo features | Optional Rust feature selection; valid only for a Rust target in a Rust package. |
| `julia` | mapping | No extension sources | Optional Julia source selection; valid only for a Julia target in a Julia package. |

Python targets select maintained source files or directories; R targets select
the package source directory, usually `.`. Julia and Rust targets must name an
explicit entry file, such as `src/MyPackage.jl` or `src/lib.rs`.

C++ packages use `ecosystem: cpp` and `extractor: doxygen`. Both
`metadata-path` and the extraction target `path` identify Doxyfiles, with paths
inside each Doxyfile resolved from the package root. See
[C++ extraction](../guide/extraction.md#cpp-with-doxygen) for setup.

### Julia target settings

Fields in `packages[].targets[].julia`:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `extensions` | list of paths | `[]` | Explicit extension `.jl` entry files relative to the package root. |

For example, `julia: { extensions: [ext/MyPackageMakieExt.jl] }` adds that
extension's static declarations to the target. Extension files and their
literal includes must remain inside the package. They retain their own module
scope, and imports resolve against the supplied core declarations. Extension
selection does not load optional dependencies or prove runtime activation.
See the [Julia extraction
contract](https://github.com/jolars/diplodocus/blob/main/docs/ir/julia-extraction.md#explicit-extension-sources).

### Rust target settings

Fields in `packages[].targets[].rust`:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `features` | list of strings | `[]` | Additional Cargo features for both extraction and doctests. |
| `default-features` | boolean | `true` | Enable the crate's default features. |

Features must be individual, nonempty Cargo feature names, without commas,
whitespace, control characters, or a leading `-`. All features are never
enabled implicitly. See [Rust extraction](../guide/extraction/rust.md).

## `content`

Each entry declares a directory or a single authored file. Collection IDs must
be unique.

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `id` | string | Required | Collection identifier. |
| `owner` | string | Required | `project` for shared pages, or a declared package ID. |
| `repository` | string | Required | Declared repository ID. |
| `path` | path | Required | Source directory or file, relative to the repository. |
| `mount` | string | Required | URL directory beneath the owner's documentation root. `""` uses that root. |
| `format` | string | Required | `gfm`, `qmd`, `rmd`, or `documenter`. |
| `navigation` | string | `all` | `all`, `index`, or `none` for automatic sidebar inclusion. |
| `execution` | mapping | Depends on format | Collection execution settings, described below. |

Formats select `.md` files for `gfm` and `documenter`, `.qmd` for `qmd`, and
`.Rmd` or `.rmd` for `rmd`. A single-file collection retains its basename and
case beneath the mount, so `CHANGELOG.md` with `mount: reference` produces
`reference/CHANGELOG.html`.

`navigation: index` includes only the collection's root index page.
`navigation: none` omits all its automatic sidebar links. These settings do not
change routes, content, search, or sitemap inclusion. Explicit sidebars can
still link to those pages. See [navigation](../guide/navigation.md).

### Collection execution

Fields in `content[].execution`:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `mode` | string | `execute` for `rmd`; `never` otherwise | `never` keeps code illustrative; `execute` requests evaluation. |
| `engine` | string | `knitr` for `rmd`; unset otherwise | `jupyter`, `knitr`, or `julia`, as allowed by the format. |
| `kernel` | string | Unset | Required Jupyter kernelspec name for executable QMD. |
| `julia-project` | path | Unset | Required prepared project file for executable Documenter pages, relative to the repository. |
| `declared-environment-inputs` | list of paths | `[]` | Additional repository-relative files recorded as execution inputs. |

The supported combinations are:

| Format | Mode | Engine | Additional requirements |
| --- | --- | --- | --- |
| `gfm` | `never` | Unset | Execution settings must be omitted. |
| `qmd` | `never` | Unset | No kernel or environment inputs. |
| `qmd` | `execute` | `jupyter` | An explicit `kernel`. |
| `documenter` | `never` | Unset | No project, kernel, or environment inputs. |
| `documenter` | `execute` | `julia` | A `julia-project`, with no kernel. |
| `rmd` | `execute` | `knitr` | No kernel or Julia project; native knitr is required. |

A kernel selector uses ASCII letters, digits, `-`, `.`, or `_`. Empty names,
`.` and `..`, and filesystem paths are invalid. The kernel must already be
installed and discoverable.

`julia-project` must name `Project.toml` or `JuliaProject.toml` within the
repository. The project file and available manifests beside it are observed
automatically.

`declared-environment-inputs` names individual existing files, not directories
or globs. Use forward slashes. Paths must stay in the repository and must be
distinct after normalization. Diplodocus records these inputs rather than
installing or interpreting them. QMD inputs also contribute to execution cache
keys. These fields are invalid with `mode: never`.

Document metadata can disable QMD execution but cannot enable it for a
collection. API doctests and R help-topic examples have their own native
execution rules and do not use these settings. See
[executable examples](../guide/execution.md).

## `presentation` {#presentation}

All fields are optional. Text is rendered as plain text rather than Markdown
or HTML.

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `title` | string | `project.name` | Site title and branding. |
| `description` | string | Unset | Site-wide HTML description and social preview description. |
| `pages` | mapping | `{}` | Head metadata overrides keyed by generated site-relative HTML path. |
| `site-url` | URL | Unset | Public site root, including any hosting prefix. Enables canonical links, `sitemap.xml`, and `robots.txt`. |
| `canonical-url-style` | string | `file` | `file` preserves HTML filenames; `clean` uses directory URLs for indexes and removes other `.html` suffixes from links. |
| `home` | mapping | Unset | Wide, centered homepage content. |
| `sections` | list | `[]` | Additional project navbar sections after Guide. |
| `header` | mapping | All public repositories | Repository links in the navbar. |
| `sidebar` | list or mapping | Automatic directory grouping | Authored navigation order and groups. |
| `assets` | list | `[]` | Public files retained at explicit publication paths. |
| `icons` | list | `[]` | Icon and manifest links on every complete HTML page. |
| `logo` | mapping | Unset | Image beside the site title. |
| `open-graph` | mapping | Unset | Shared social preview image. Requires `site-url`. |
| `redirects` | mapping | `{}` | Permanent redirects from old URL paths to generated pages. |
| `fragment-aliases` | mapping | `{}` | Legacy fragment IDs attached to maintained headings and API targets, keyed by generated HTML path. |
| `footer` | mapping | No footer | Documentation version, notices, and links. |

`site-url` must be an absolute HTTP or HTTPS URL without credentials, query
parameters, fragments, or whitespace. Diplodocus treats it as a directory root.
Clean URLs require hosting support; output filenames keep their `.html`
extensions. Ambiguous clean routes fail generation. See
[site appearance](../guide/presentation.md) for examples and deployment details.

### Header

`presentation.header.repositories` accepts a list of declared repository IDs in
display order. Omit it to include all repositories with public HTTP or HTTPS
URLs, set `[main]` to link only the repository named `main`, or set `[]` to hide
repository links. Repositories without public URLs are omitted, and repeated
URLs appear once.

One repository appears as a direct link, using the GitHub icon for GitHub URLs.
Multiple repositories appear in a **Repositories** dropdown with named links.
On phones, those links appear inside the navigation menu.

### Page metadata

Each entry in `presentation.pages` names a generated HTML path, such as
`packages/python/index.html`, without a leading slash or hosting prefix. The
path includes `.html` with either URL style and must name an existing page.

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `title` | string | Page title with site suffix | Complete HTML and social card title. No suffix is added to overrides. Must not be blank. |
| `description` | string | `presentation.description` | HTML and social card description. An empty string emits empty description tags. |

Overrides affect head metadata. Headings, navigation, search, and Markdown keep
their existing titles. Without an override, matching page and site titles appear
once. See [page titles and
descriptions](../guide/presentation.md#page-titles-and-descriptions) for
precedence and examples.

### Homepage

Fields in `presentation.home`:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `tagline` | string | Required | Main heading; newlines separate heading lines. |
| `description` | string | Required | Introduction below the heading. |
| `guide` | HTML route | Required | Visible page opened by the navbar's Guide link. |
| `actions` | list | `[]` | Call-to-action links in display order; the first is emphasized. |
| `features` | list | `[]` | Linked feature cards in display order. |

Each `presentation.home.actions` entry accepts:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `label` | string | Required | Visible action text. |
| `path` | HTML route | Required | Visible generated page. |
| `icon` | string | Unset | `arrow-right`, `book-open`, `code`, `download`, or `terminal`. |

Each `presentation.home.features` entry accepts:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `title` | string | Required | Feature heading. |
| `description` | string | Required | Feature explanation. |
| `path` | HTML route | Required | Visible generated page. |

HTML routes are site-relative generated paths such as `guide/index.html`,
without a leading slash. Include the extension even with clean URLs.
An authored root page appears below the homepage content.

### Navbar sections

Each `presentation.sections` entry accepts:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `label` | string | Required | Navbar and automatic sidebar heading. |
| `path` | HTML route | Required | Visible project `index.html` page defining the section. |

All project pages under that directory belong to the section. Section
directories must be distinct and cannot contain the Guide landing page. Entries
appear in declaration order, before package tabs.

### Sidebar {#sidebar}

`presentation.sidebar` accepts a list used for every page or a mapping from
site-root directory prefixes to lists. Prefixes begin and end with `/`; the
longest matching prefix wins. `/` provides a fallback. An empty list removes
authored navigation, and unmatched pages retain automatic navigation. Prefixes
do not include a hosting prefix from `site-url`.

Each sidebar item, including nested `items`, accepts:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `text` | string | Required | Visible label. |
| `link` | HTML route | Unset | Existing, visible generated page. |
| `items` | list | `[]` | Child links or groups, in display order. |
| `collapsed` | boolean | Unset | `true` starts closed; `false` adds a toggle that starts open. Omission keeps the group expanded without a toggle. |

An item needs a `link`, nonempty `items`, or both. A group can link to its own
landing page. Nesting is limited to six levels, including the top level. Links
use generated HTML routes without a leading slash; external URLs, fragments,
and source Markdown paths are invalid. The current page's ancestors open
automatically. Explicit sidebars replace automatic authored and shared-concept
groups; package API trees still appear. See
[navigation](../guide/navigation.md) for examples.

### Public assets and icons

Fields in each `presentation.assets` entry:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `repository` | string | Required | Declared repository containing the file. |
| `path` | path | Required | Normalized repository-relative source file path. |
| `publish` | path | Required | Normalized site-relative publication path, without a hosting prefix. |

Paths cannot contain empty or dot segments, backslashes, percent escapes,
colons, query parameters, or fragments. Files cannot collide with one another,
generated pages and their Markdown companions, clean URLs, redirects, or
reserved outputs: `assets/`, `__diplodocus/`, `.diplodocus-site`, `_redirects`,
`sitemap.xml`, `robots.txt`, and `documents.json`.

Fields in each `presentation.icons` entry:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `path` | path | Required | A publication path declared in `presentation.assets`. |
| `rel` | string | `icon` | `icon`, `apple-touch-icon`, or `manifest`. |
| `type` | string | Unset | MIME type hint matching the publication extension. |
| `sizes` | string | Unset | `any` or space-separated positive dimensions, such as `16x16 32x32`. Not allowed for manifests. |

Icon links require an image extension: `.svg`, `.ico`, `.png`, `.jpg`, `.jpeg`,
`.webp`, `.gif`, or `.avif`. Manifest links require `.webmanifest` or `.json`.
Response MIME types follow publication extensions. Other public files may use
any extension; unknown extensions use `application/octet-stream`. Files retain
their original bytes. See [public assets and icons](../guide/presentation.md#public-assets-and-icons).

### Logo

Fields in `presentation.logo`:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `repository` | string | Required | Repository containing the logo. |
| `path` | path | Required | Repository-relative SVG, PNG, or JPEG file. |

Image paths use forward slashes without a leading slash or `.` or `..`
components. Symlinks must stay in the repository. Images may live outside
content collections and are retained in snapshots.

### Open Graph image

Fields in `presentation.open-graph`:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `repository` | string | Required | Repository containing the preview image. |
| `path` | path | Required | Repository-relative PNG or JPEG file. |
| `alt` | string | Required | Nonempty accessible description of the image. |

The image follows the logo's path rules and requires `presentation.site-url`.
Generated HTML pages receive Open Graph metadata and a large-image Twitter
card using their own titles and canonical URLs.

### Redirects

`presentation.redirects` maps exact old URL paths to visible generated HTML
routes. Keys start with `/` and omit any hosting prefix. Values include
`.html`, without a leading slash. Each old spelling, including trailing
slashes, needs its own entry.

Sources cannot replace current pages, Markdown exports, or reserved output
paths. Wildcards, external destinations, query-specific rules, fragments,
chains, and loops are unsupported. Paths must not contain encoded separators,
control characters, or dot segments. Destinations follow
`canonical-url-style`. The standalone site writes `_redirects`, with at most
2,000 rules and 1,000 characters per rule. `serve` applies the same HTTP 301
redirects locally. Other hosts may need equivalent server configuration. See
[redirect examples](../guide/presentation.md#redirects-after-moving-pages).

### Fragment aliases

`presentation.fragment-aliases` maps generated site-relative HTML paths to
legacy fragment IDs and their maintained targets. Paths include `.html` and omit
the leading slash and hosting prefix, including for clean URLs:

```yaml
presentation:
  fragment-aliases:
    guide/start.html:
      Getting-Started: { heading: Getting started }
      autotoc_md0: { anchor: installation }
```

Each target contains exactly one field: `heading` or `anchor`. `heading` matches
the visible plain text of an authored or generated title, section, or API member
heading. Inline formatting is removed, and whitespace is collapsed before
matching. Matching is case-sensitive. Repeated heading text is ambiguous; use
`anchor` to select its canonical heading or API ID instead. UI labels and
headings in generated execution output are not maintained targets.

IDs are literal strings, without URL encoding. Preserve their case, Unicode,
punctuation, and spaces, including spaces in legacy Julia method signatures.
Empty IDs and control characters are rejected. Encode IDs when constructing
URLs, such as `%20` for a space. Multiple aliases may select one target.

Missing pages or targets, ambiguous headings, duplicate IDs, and collisions with
existing document, API, or site-shell IDs are errors. An alias identical to its
target's canonical ID already works and emits no additional marker. Aliases
never replace canonical IDs or change generated heading slugs, TOCs, search, or
fragment-manifest heading entries.

HTML retains aliases inside their targets, and Markdown exports retain an anchor
before each target. Aliases work with route redirects, native preview, and
fragment generation. Snapshots retain the declarations, so detached generation
needs no source checkout. See the [migration
example](../guide/presentation.md#preserving-old-fragments).

### Footer

Fields in `presentation.footer`:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `docs-version` | string | Unset | Free-form documentation release or branch label. |
| `copyright` | string | Unset | Complete notice, including symbol, years, and holder. |
| `licenses` | list | `[]` | Alternative content licenses, joined with "or". |
| `links` | list | `[]` | Project links in display order. |
| `built-with` | boolean | `false` | Show a linked "Built with Diplodocus" credit. |

Each entry in `presentation.footer.licenses` or `presentation.footer.links`
accepts:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `label` | string | Required | Visible link text. |
| `url` | URL | Required | Absolute HTTP or HTTPS destination without credentials or whitespace. |

Diplodocus does not infer a documentation version or copyright from package
metadata or the build date.

## `components` {#components}

Each entry declares a prebuilt browser bundle:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `id` | string | Required | Name used in authored component blocks. |
| `repository` | string | Required | Repository containing the bundle. |
| `path` | path | Required | Repository-relative bundle directory. |
| `module` | path | Required | Bundle-relative ES module exporting `mount`. |
| `stylesheets` | list of paths | `[]` | Bundle-relative stylesheets loaded before mounting. |

Build bundles before checking or extracting documentation. Bundle files must
be regular files within the directory; symlinks are rejected. Configuration
builds enable declared bundles. Generation from a trusted snapshot requires
`--allow-components`. See [browser components](../guide/authoring/components.md)
for authoring and lifecycle details.

## `concepts`

Each entry groups corresponding API items. Concept IDs must be unique.

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `id` | string | Required | Stable concept identifier. |
| `kind` | string | Required | `equivalent`, `analogous`, or `related`. |
| `members` | list | Required | Corresponding API items, in declaration order. |

`equivalent` means the same API, `analogous` means comparable capabilities, and
`related` records an explicit association. Each `concepts[].members` entry
accepts:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `package` | string | Required | Declared package ID. |
| `item` | string | Required | Semantic item ID or an unambiguous qualified name. |

## `relationships`

Each entry records a package relationship, separate from an API concept:

| Option | Type | Default | Purpose |
| --- | --- | --- | --- |
| `from` | string | Required | Source package ID or external `ecosystem:package` coordinate. |
| `to` | string | Required | Destination package ID or external `ecosystem:package` coordinate. |
| `kind` | string | Required | `depends-on`, `binds`, `wraps`, or `generated-from`. |
| `version-constraint` | string | Unset | Version constraint retained in the ecosystem's syntax. |
| `provenance` | string | Unset | Declaration origin, such as `explicit`. |

`from` is the package that depends on, binds, wraps, or is generated from `to`.
Endpoints first match workspace package IDs. Other endpoints require an
explicit ecosystem prefix, such as `python:numpy` or `r:stats`. The ecosystem
starts with an ASCII letter and then uses letters, digits, `.`, `_`, or `-`.
Colon-separated components must be nonempty, with no whitespace or control
characters. Diplodocus retains external coordinates without resolving packages
or inferring unknown unqualified names.
