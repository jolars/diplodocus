# API extraction

Diplodocus reads the API targets you declare and publishes reference pages
alongside your guides. In [project configuration](configuration.md#add-a-package),
give each package a source directory, a metadata file, and an extraction target.
Keep those inputs inside the declared checkout. Prepare the tools and
dependencies each language guide lists before you build.

## Python

Use the [Python guide](extraction/python.md) to document a prepared package
checkout, including public reexports, stub signatures, and doctests.

## R

Use the [R guide](extraction/r.md) to generate help topics with roxygen2 or use
maintained Rd files. Diplodocus also picks up package vignettes.

## Julia

Use the [Julia guide](extraction/julia.md) to document the loaded package's
bindings and methods, verify doctests, and bring in Documenter pages.

## Rust

Use the [Rust guide](extraction/rust.md) to prepare the pinned nightly toolchain
and run rustdoc against your library.

## C++ with Doxygen {#cpp-with-doxygen}

Use the [C++ guide](extraction/cpp.md) to document headers and other sources
through an existing or minimal Doxyfile.
