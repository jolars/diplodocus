# Getting started

This tutorial builds a small documentation site with two Markdown pages. You
need the [Diplodocus CLI](installation.md) and a text editor. You can add API
references afterward, so no Python, R, Julia, or Rust environment is needed for
this first site.

## Create your project

Create a directory with this structure:

```text
my-project/
├── diplodocus.yaml
└── docs/
    ├── index.md
    └── tutorial.md
```

In `diplodocus.yaml`, declare the project and its documentation directory:

```yaml
project:
  name: "My project"
repositories:
  - id: "project"
    path: "."
content:
  - id: "guide"
    owner: "project"
    repository: "project"
    path: "docs"
    mount: ""
    format: "gfm"
```

The repository path `.` means the directory containing the configuration.
The collection reads Markdown files from `docs`. Its empty `mount` puts the
pages at the site root.

## Write two pages

Add a title, a short introduction, and a link to `docs/index.md`:

```markdown
# My project

Welcome to the documentation.

Read the [tutorial](tutorial.md) to get started.
```

Then add `docs/tutorial.md`:

```markdown
# Tutorial

Explain the first task your readers want to complete.

Return to the [overview](index.md).
```

Diplodocus uses the page headings as titles and turns the relative Markdown
links into site links. Both pages appear in the sidebar and search.

## Check and preview

From `my-project/`, run:

```console
diplodocus check
diplodocus serve --live-reload
```

Open the local URL printed in the terminal, normally
`http://127.0.0.1:8000/`. Edit `docs/tutorial.md` and save it. Diplodocus
rebuilds the site and refreshes the browser after a successful build. Stop the
preview with Ctrl-C.

## Build for publishing

Run:

```console
diplodocus build
```

The generated site is in `site/`, starting at `site/index.html`. Upload that
directory to your static host. Use `--output PATH` to choose another output
directory. When you know the public address, set `presentation.site-url` as
described in [site appearance](presentation.md).

## Add your packages

Keep this guide and add package declarations to the same configuration. Follow
the [configuration guide](configuration.md#add-a-package), then the extraction
guide for [Python](extraction/python.md), [R](extraction/r.md),
[Julia](extraction/julia.md), or [Rust](extraction/rust.md).

For a complete project using all four languages, see the
[monorepo example](https://github.com/jolars/diplodocus/tree/main/examples/monorepo).
Continue with [writing pages](authoring.md) and [navigation](navigation.md) to
shape the guide for your readers.
