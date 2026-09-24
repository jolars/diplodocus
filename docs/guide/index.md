# Diplodocus

Diplodocus brings related Python and R packages into one documentation site.
Authored guides, API references, navigation, conceptual API groups, and search
share the same workspace.

The project is under development. `check` validates sources without execution.
`build` executes authorized Python and R pages, publishes a portable SQLite
snapshot, and generates the site. `serve` watches declared inputs and keeps the
last successful site available if a rebuild fails.

Start with the [quick start](quick-start.md), read about
[workspace configuration](configuration.md), or consult the [commands](cli.md).
The [executable example](../examples/stateful.qmd) provides a small deterministic
document for testing the project's own site.

![A documentation workspace with Python and R packages](assets/workspace.svg)

## One explicit workspace

A configuration declares repository roots, packages, authored collections, and
relationships. Python and R APIs are parsed statically without importing or
loading the documented packages. Executing authored examples requires an
explicitly enabled QMD collection and an installed Jupyter kernel.

Diplodocus does not install dependencies or implicitly access the network.
Generated sites use local assets and portable, repository-relative source
locations.
