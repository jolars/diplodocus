# Navigation

Choose the order and groups readers see in the sidebar, and use navbar sections
for distinct parts of your site. You can arrange pages without renaming their
source files or changing their URLs.

## Start with an ordered sidebar

Add `presentation.sidebar` to your configuration. This example keeps the first
pages in a fixed order and places troubleshooting in a group that starts closed:

```yaml
presentation:
  sidebar:
    - text: Introduction
      items:
        - text: Overview
          link: guide/index.html
        - text: Getting started
          link: guide/quick-start.html
        - text: Installation
          link: guide/installation.html
    - text: Advanced
      collapsed: true
      items:
        - text: Troubleshooting
          link: guide/diagnostics.html
```

Use routes that exist on your site. Labels and order come from this list;
links name generated HTML paths, even when your host uses clean URLs. The
current page's group opens automatically.

## Control automatic navigation

Collections include every page in the sidebar by default. For a generated catalog
whose index already links to its entries, show only the collection's root index:

```yaml
content:
  - id: datasets
    owner: project
    repository: docs
    path: generated/datasets
    mount: datasets
    format: gfm
    navigation: index
```

`navigation` accepts `all` (the default), `index` (only `index.md`, `index.qmd`,
or `index.Rmd` at the collection root), and `none` (no sidebar links). The setting
applies to project and package collections. It does not change page routes,
rendered content, links, search entries, or sitemap inclusion. If the collection
has no root index, `index` adds no sidebar links. The setting is stored in the
snapshot, so `generate` retains it without the source checkout.

To give a project collection its own navbar tab and sidebar, declare a section:

```yaml
presentation:
  sections:
    - label: "Reference"
      path: "reference/index.html"
```

The path names a generated, visible project `index.html` page. All project pages
under that directory belong to the section and leave the Guide sidebar.
Section directories must be distinct and cannot contain the Guide landing page.
Tabs appear after Guide in declaration order, before package tabs.

## Sidebar groups and nesting

Without an explicit sidebar, Diplodocus groups authored pages by their generated
URL directories. The first directory level creates headings, and deeper pages
appear in the same group. A directory's index page supplies the heading and
appears first as **Overview**; otherwise, the directory name supplies the
heading. Other pages follow in route order.

For explicit ordering, nested groups, and collapsible headings, set
`presentation.sidebar`. Its item structure follows [VitePress's sidebar
configuration](https://vitepress.dev/reference/default-theme-sidebar):

```yaml
presentation:
  sidebar:
    - text: Introduction
      collapsed: false
      items:
        - text: Overview
          link: guide/index.html
        - text: Getting started
          link: guide/quick-start.html
    - text: Reference
      items:
        - text: Configuration
          link: reference/configuration.html
        - text: Authoring
          items:
            - text: Overview
              link: guide/authoring.html
            - text: Executable examples
              link: guide/execution.html
```

Every item needs `text` and either a `link`, nonempty `items`, or both. A group
with a link has a clickable heading. Links name generated HTML routes relative
to the site root, without a leading slash, even when the site uses clean URLs or
a hosting prefix. They must identify existing, visible pages. External URLs,
fragments, and source paths such as `guide/index.md` are not accepted.

Top-level groups appear beneath headings. Nested groups indent their children
and add a vertical guide line. Items retain declaration order and may nest up to
six levels, including the top level. Deeper trees fail validation.

Omitting `collapsed` creates an always-expanded group. Set `collapsed: false` to
add a toggle that starts open, or `collapsed: true` to start closed. The current
page's ancestor groups open automatically. Toggles work without JavaScript.
During client-side navigation, Diplodocus remembers expanded and collapsed
groups within each sidebar until the page is reloaded.

## Different sidebars by directory

A sidebar list applies to every page. To configure only particular directories,
use a map instead:

```yaml
presentation:
  sidebar:
    /guide/:
      - text: Introduction
        collapsed: false
        items:
          - text: Overview
            link: guide/index.html
          - text: Getting started
            link: guide/quick-start.html
    /reference/:
      - text: Reference
        items:
          - text: Overview
            link: reference/index.html
```

Map keys start and end with `/` and refer to generated URL directories,
independent of any hosting prefix. The longest matching directory wins. `/`
provides a site-wide fallback; pages without a matching key keep automatic
navigation. An empty list explicitly removes authored navigation for that
sidebar. Package directories such as `/packages/python/` also work.

An explicit sidebar replaces automatic authored groups and the automatic shared
concepts group for matching pages. Package API trees still appear below it.
Explicit links may include pages omitted by a collection's `navigation` setting;
that setting controls automatic inclusion. Pages left out of the sidebar still
render and remain searchable. Sidebar settings are stored in the snapshot, so
`generate` can reproduce them without source checkouts.


See [site appearance](presentation.md) for the homepage and branding, or the
[configuration reference](../reference/configuration.md#sidebar) for every
sidebar field.
