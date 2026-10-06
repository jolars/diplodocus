# Diplodocus

[![CI](https://github.com/jolars/diplodocus/actions/workflows/ci.yml/badge.svg)](https://github.com/jolars/diplodocus/actions/workflows/ci.yml)

Diplodocus builds one documentation site for related Python and R packages.
It combines statically extracted APIs with authored Markdown and QMD guides,
shared navigation, semantic cross-references, and search. The project is under
active development; the [current status and remaining work](TODO.md) describe
what still needs attention.

## Start here

The [Diplodocus guide](https://jolars.github.io/diplodocus/) is built from this
repository with Diplodocus. The [small R/Python monorepo](examples/monorepo/README.md)
is the quickest way to build a complete package site locally. From the
repository root:

```console
devenv shell
task preview
```

Open the local URL printed by the command. This example builds without Python,
R, or Jupyter kernels at runtime. The [quick start](docs/guide/quick-start.md)
shows direct CLI commands, Diplodocus's own documentation site, and the broader
acceptance workspace.

## How it works

A `diplodocus.toml` file declares the repositories, packages, and authored
collections in a workspace. `check` validates declared sources without
executing cells. `extract` publishes a portable SQLite snapshot, `generate`
renders a site from a completed snapshot, and `build` combines those stages.
`serve` watches declared inputs and keeps the last successful site available
when a rebuild fails. QMD execution requires explicit collection authority and
an installed kernel.

Read the [configuration guide](docs/guide/configuration.md), [command
reference](docs/guide/cli.md), or [snapshot schema](docs/design/snapshot-schema.md)
for details. The [Python](docs/ir/python-extraction.md) and
[R](docs/ir/r-extraction.md) contracts describe the static extraction surface.

## Development

Use `devenv shell` for the pinned Rust, Python, R, Node, and browser tools. Run
`npm ci --ignore-scripts` after checkout or a change to `package-lock.json`.
The [contributor instructions](AGENTS.md) give the code map and verification
commands. The [browser workflow](docs/development/browser.md) covers `site-dev`,
`site-test`, and `site-capture`.

To inspect the CLI from this checkout:

```console
cargo run --locked -- --help
```

## Documentation publishing

The `Documentation` workflow checks and builds this repository's Diplodocus
site on pull requests and `main`. A `v*` tag, or a manual dispatch on `main`,
also deploys the generated site to GitHub Pages. In the repository's Pages
settings, select **GitHub Actions** as the publishing source before the first
deployment. The site is served beneath `/diplodocus/`; the project-site test
checks its links and search paths under that prefix.

## Releases

Conventional commits determine releases. Versionary opens and maintains the
release pull request, updates `Cargo.toml`, `Cargo.lock`, and `CHANGELOG.md`, and
creates the version tag and GitHub release after that pull request is merged.

Repository administrators must configure:

- A `RELEASE_TOKEN` repository secret containing a fine-grained personal access
  token, bot token, or GitHub App token with read/write access to contents, pull
  requests, and issues. A token other than the workflow `GITHUB_TOKEN` is
  required so Versionary-created tags trigger the publishing workflow.
- A protected GitHub Actions environment named `release`, with the desired
  deployment reviewers and branch or tag restrictions.
- A crates.io trusted publisher for `jolars/diplodocus`, the
  `.github/workflows/publish-crates.yml` workflow, and the `release`
  environment.

Publishing runs only for an explicit `v*` tag or a manual workflow dispatch and
uses `cargo publish --locked` with crates.io's short-lived OIDC token.

## License

Licensed under either the Apache License, Version 2.0, or the MIT license, at
your option. See [`LICENSE-APACHE`](LICENSE-APACHE) and
[`LICENSE-MIT`](LICENSE-MIT).

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this project is dual licensed as above, without any
additional terms or conditions.
