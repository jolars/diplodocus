# Configuration

Use `diplodocus.yaml` to tell Diplodocus which sources belong on your site.
Start with the small configuration in [getting started](quick-start.md), then
add packages and collections as your documentation grows. This guide explains
the main choices; the [configuration reference](../reference/configuration.md)
lists every option, default, and required field.

## Add pages {#add-pages}

Name the project and select a directory of Markdown pages beside
`diplodocus.yaml`:

```yaml
project:
  name: My project
content: docs
```

This publishes the contents of `docs` at the site root. To select individual
sources or change where they publish, give each source a path and URL prefix:

```yaml
content:
  - path: docs
    mount: guide
  - path: CHANGELOG.md
    mount: development
```

Here, `docs/installation.md` becomes `guide/installation.html` and
`CHANGELOG.md` becomes `development/CHANGELOG.html`. An empty mount publishes
at the site root. Paths are relative to the configuration directory and must
stay in the local checkout. Set `project.repository-url` to add links to the
checkout on GitHub.

Collections use GFM by default. Set `format` to `qmd` for Quarto Markdown,
`rmd` for R Markdown, or `documenter` for Julia Documenter pages. See
[writing pages](authoring.md) for examples.

## Add a package {#add-a-package}

Declare the package and an explicit API extraction target. This example adds a
Python package in the repository's `python/` directory:

```yaml
packages:
  - id: "python"
    name: "My Python package"
    slug: "python"
    ecosystem: "python"
    path: "python"
    metadata-path: "pyproject.toml"
    targets:
      - id: "api"
        extractor: "python"
        path: "src/mypackage"
        role: "public-api"
```

The package `path` is relative to the configuration directory by default.
`metadata-path` and each target's `path` are relative to the package. The `id` identifies the package in
links and other configuration; the `slug` places its documentation under
`packages/python/`.

If the package lives in another checkout, declare it under `repositories`
and select it with `packages[].repository`. Add package-owned pages under the
package's `content` list. Their source paths start at the package root and
their mounts start at the package's documentation URL.

Use the [Python](extraction/python.md), [R](extraction/r.md),
[Julia](extraction/julia.md), [Rust](extraction/rust.md), or
[C++](extraction/cpp.md) guide to choose the metadata file, target, and required
environment for your package. Declare
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
Use the [collection execution settings](../reference/configuration.md#collection-execution)
when adding runnable pages.

## Validate your changes

Run `diplodocus check` after changing the configuration. It checks declarations,
source paths, and links without running documentation examples. To use another
configuration file, run `diplodocus check --config path/to/diplodocus.yaml`.

Field names use kebab-case, such as `metadata-path`. Unknown fields, duplicate
keys, and multiple YAML documents are errors. Keep only the settings your site
needs, and look up less common options in the
[configuration reference](../reference/configuration.md).
