# Diplodocus <img src="https://raw.githubusercontent.com/jolars/diplodocus/refs/heads/main/assets/brand/diplodocus-logo-small.png" align="right" width="139" alt="Diplodocus writing in an open book" />

[![CI](https://github.com/jolars/diplodocus/actions/workflows/ci.yml/badge.svg)](https://github.com/jolars/diplodocus/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/diplodocus.svg)](https://crates.io/crates/diplodocus)
[![npm](https://img.shields.io/npm/v/diplodocus)](https://www.npmjs.com/package/diplodocus)
[![PyPI](https://img.shields.io/pypi/v/diplodocus?label=PyPI)](https://pypi.org/project/diplodocus/)

Diplodocus builds a shared documentation site for Python, R, Julia, Rust, and
C++ packages. It brings package API references and Markdown and Quarto guides
together with shared navigation, cross-references, and search.

## Installation

Install from PyPI with `uv tool install diplodocus` or
`pipx install diplodocus`. With npm, run `npm install --global diplodocus`.

On Linux or macOS, install the latest release with:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://diplodocus.cc/install | sh
```

On Windows, run this in PowerShell:

```powershell
irm https://diplodocus.cc/install.ps1 | iex
```

Prebuilt binaries are available from [GitHub
releases](https://github.com/jolars/diplodocus/releases). See the [installation
guide](https://diplodocus.cc/guide/installation) for requirements and other
installation options.

## Getting started

From an R package checkout, run `diplodocus serve` to preview its package
overview, reference, and vignettes. For other projects or to customize the site,
create a `diplodocus.yaml` file. Use the [configuration guide](https://diplodocus.cc/guide/configuration)
to set up your project, or adapt the [Python, R, Julia, and Rust
example](examples/monorepo/README.md).

From the directory containing your configuration, validate your sources and
preview the site:

```sh
diplodocus check
diplodocus serve
```

Open the local URL printed in the terminal. Diplodocus watches your sources and
rebuilds the site when they change. To build a site for publishing, run:

```sh
diplodocus build
```

Install the language runtimes and package dependencies your documentation needs
before building.

## Documentation

- [Diplodocus documentation](https://diplodocus.cc/)
- [Configuration](https://diplodocus.cc/guide/configuration)
- [Writing documentation](https://diplodocus.cc/guide/authoring)
- [Command reference](https://diplodocus.cc/reference/cli)
- [Design](https://diplodocus.cc/development/design)
- [Contributing](https://diplodocus.cc/development/CONTRIBUTING)

## License

Diplodocus is licensed under either the [Apache License, Version
2.0](LICENSE-APACHE), or the [MIT license](LICENSE-MIT), at your option.
