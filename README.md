# Diplodocus <img src="https://raw.githubusercontent.com/jolars/diplodocus/refs/heads/main/assets/brand/diplodocus-logo-small.png" align="right" width="139" alt="Diplodocus writing in an open book" />

[![CI](https://github.com/jolars/diplodocus/actions/workflows/ci.yml/badge.svg)](https://github.com/jolars/diplodocus/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/diplodocus.svg)](https://crates.io/crates/diplodocus)

Diplodocus builds a shared documentation site for Python, R, Julia, and Rust packages.
It brings package API references and Markdown and Quarto guides together with
shared navigation, cross-references, and search.

## Installation

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
guide](docs/guide/installation.md) for requirements and other installation
options.

## Getting started

Create a `diplodocus.toml` file to declare your packages and documentation
pages. Use the [configuration guide](docs/guide/configuration.md) to set up your
project, or adapt the [Python, R, Julia, and Rust
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
before building. See the [quick start](docs/guide/quick-start.md) for a complete
example.

## Documentation

- [Diplodocus documentation](https://diplodocus.cc/)
- [Configuration](docs/guide/configuration.md)
- [Writing documentation](docs/guide/authoring.md)
- [Command reference](docs/guide/cli.md)

## License

Diplodocus is licensed under either the [Apache License, Version
2.0](LICENSE-APACHE), or the [MIT license](LICENSE-MIT), at your option.
