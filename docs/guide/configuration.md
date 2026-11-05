# Configuration

Use `diplodocus.yaml` to tell Diplodocus which sources belong on your site.
Start with the small configuration in [getting started](quick-start.md), then
add packages and collections as your documentation grows. This guide explains
the main choices; the [configuration reference](../reference/configuration.md)
lists every option, default, and required field.

## Name the project and its repositories

The project name appears in the site header. A repository identifies a local
source checkout. For a project whose configuration lives at its repository
root, use:

```yaml
project:
  name: "My project"
repositories:
  - id: "core"
    path: "."
    url: "https://github.com/example/my-project"
```

`path` is relative to the configuration file. It can also point to a sibling
checkout, so one site can document packages from several repositories. Other
declarations refer to a repository by its `id`. The optional `url` provides
links back to your source repository.

## Add pages

A content collection declares a directory or individual file to publish:

```yaml
content:
  - id: "guide"
    owner: "project"
    repository: "core"
    path: "docs"
    mount: "guide"
    format: "gfm"
```

Here, `path` is relative to the `core` repository, and `mount` is a URL prefix.
For example, `docs/installation.md` becomes `guide/installation.html`. An empty
mount puts pages at the site's root. The `project` owner makes this a shared
guide; a package ID makes the collection part of that package's documentation.

Use `gfm` for ordinary Markdown, `qmd` for Quarto Markdown, `rmd` for R Markdown,
or `documenter` for Julia Documenter pages. Start with GFM unless you need
another format's features. See [writing pages](authoring.md) for examples.

## Add a package {#add-a-package}

Declare the package and an explicit API extraction target. This example adds a
Python package in the repository's `python/` directory:

```yaml
packages:
  - id: "python"
    name: "My Python package"
    slug: "python"
    ecosystem: "python"
    repository: "core"
    path: "python"
    metadata-path: "pyproject.toml"
    targets:
      - id: "api"
        extractor: "python"
        path: "src/mypackage"
        role: "public-api"
```

The package `path` is relative to the repository. `metadata-path` and each
target's `path` are relative to the package. The `id` identifies the package in
links and other configuration; the `slug` places its documentation under
`packages/python/`.

Use the [Python](extraction/python.md), [R](extraction/r.md),
[Julia](extraction/julia.md), or [Rust](extraction/rust.md) guide to choose the
metadata file, target, and required environment for your package. Declare
`targets: []` if you want a package landing page and authored documentation
without API extraction. Diplodocus does not discover undeclared packages or
infer extraction targets.

By default, a package landing page uses its `README.md`. Set `overview` to
choose another Markdown file, or add an authored package index. See
[package landing pages](authoring.md#package-landing-pages).

## Arrange and style the site

Add `presentation.sidebar` to choose the order and groups in your navigation.
Add `presentation.sections` when you want a navbar tab such as Reference.
The [navigation guide](navigation.md) walks through both.

Use `presentation` for the site title, logo, homepage, footer, and public URL.
The [site appearance guide](presentation.md) provides examples you can adapt.
Execution settings belong to each content collection; follow
[executable examples](execution.md) when adding runnable pages.

## Validate your changes

Run `diplodocus check` after changing the configuration. It checks declarations,
source paths, and links without running documentation examples. To use another
configuration file, run `diplodocus check --config path/to/diplodocus.yaml`.

Field names use kebab-case, such as `metadata-path`. Unknown fields, duplicate
keys, and multiple YAML documents are errors. Keep only the settings your site
needs, and look up less common options in the
[configuration reference](../reference/configuration.md).
