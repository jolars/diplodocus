# Workspace configuration

A `diplodocus.toml` file declares the repositories, packages, and authored
content that belong in one site. Diplodocus does not discover nearby packages
or documents. Start from the working `examples/monorepo/diplodocus.toml`
configuration and adapt its paths and names. The root `diplodocus.toml`
shows an authored documentation site without API packages.

## Repositories and packages

Each repository path is relative to the configuration directory; it may point
to a sibling checkout. A package names its repository and gives a path within
it. Package metadata and extraction targets are relative to the package. Declare
`targets = []` for a package with no API extraction.

Python extraction reads source and package metadata without importing code. R
extraction reads source, package metadata, and checked-in Rd files without
starting R. Their supported inputs and diagnostics are documented in
`docs/ir/python-extraction.md` and `docs/ir/r-extraction.md`.

## Authored content

Each `[[content]]` collection names a repository, source directory, mount,
format, and owner. The `project` owner puts pages in project navigation; a
package ID puts them under `/packages/<slug>/`. GFM collections read `.md` files.
QMD collections read `.qmd` files and can include executable cells. Checked-in
assets can live beside the pages that use them.

The monorepo example has one shared
GFM guide and two statically extracted packages. The root configuration has a
GFM guide and a QMD example collection.

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
declared_environment_inputs = ["devenv.lock"]
```

Declared environment inputs are individual files relative to the repository.
They contribute to provenance and execution cache keys; Diplodocus does not
install or interpret them. Document metadata can restrict execution but cannot
enable it when the collection forbids it. `check` never runs cells. `extract`,
`build`, and `serve` may run authorized cells with your user permissions.

## Presentation

An optional table controls the site name and HTML description:

```toml
[presentation]
title = "Foo documentation"
description = "Guides and API documentation for Foo."
```

The title defaults to `project.name`. Diplodocus uses its built-in theme.
Generated sites and portable snapshots keep the presentation metadata, so
`generate` does not need the original configuration.

Return to the [quick start](quick-start.md) or see the [commands](cli.md).
