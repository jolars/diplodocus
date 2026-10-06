# Diplodocus

Diplodocus brings related Python and R packages into one documentation site.
Authored guides, API references, navigation, conceptual API groups, and search
share an explicitly configured workspace.

Start with the [small R/Python example](quick-start.md), then read about
[workspace configuration](configuration.md) and [commands](cli.md). The
[executable example](../examples/stateful.qmd) belongs to Diplodocus's own
documentation site and requires the configured Python kernel.

![A documentation workspace with Python and R packages](assets/workspace.svg)

Python and R APIs are parsed statically without importing or loading the
documented packages. Authored QMD cells run only when their collection enables
execution and the selected Jupyter kernel is installed. Diplodocus uses local
assets and does not install dependencies for a workspace.
