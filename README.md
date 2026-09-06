# Polydoc

[![CI](https://github.com/jolars/polydoc/actions/workflows/ci.yml/badge.svg)](https://github.com/jolars/polydoc/actions/workflows/ci.yml)

Polydoc builds one coherent documentation website for software projects that
span multiple programming languages and repositories. The first release will
support Python and R packages, authored Markdown, shared navigation, semantic
cross-references, and workspace-wide search.

Polydoc is under active development. The command-line surface is present, but
the build pipeline will arrive in the later milestones described in
[`TODO.md`](TODO.md).

## Authored documents

The library parses authored `.md` and `.qmd` content in-process through
`panache-parser`. Use `documents::parse_authored_document` with the `Gfm` or
`Qmd` profile to obtain Polydoc's serializable document IR and source-ordered
diagnostics. The current QMD adapter extracts executable cells and their
options, but does not execute them.

The supported authored subset covers prose, headings, lists, links and images,
pipe tables, GFM alerts, QMD frontmatter, QMD callouts, and executable fences
with hashpipe YAML. A code-only unresolved reference such as
`` [`package::item`] `` becomes a semantic reference. Other unsupported syntax
is retained as an explicit IR node and produces a warning rather than being
silently discarded.

## Development

Enter the reproducible development shell:

```console
devenv shell
```

The shell supplies the pinned Rust toolchain, Python, R, and the project-wide
development tools. Run the local checks with:

```console
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps
cargo audit
cargo deny check
actionlint .github/workflows/*.yml
```

Generate a coverage report with:

```console
cargo llvm-cov --locked --all-targets
```

Inspect the command-line interface from the checkout with:

```console
cargo run --locked -- --help
cargo run --locked -- build --help
cargo run --locked -- check --help
cargo run --locked -- serve --help
```

Pre-commit hooks installed by devenv run rustfmt and all-target, all-feature
Clippy with warnings denied. Intentional CLI golden-file changes can be accepted
with `SNAPSHOTS=overwrite cargo test --test cli` after reviewing the diff.

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
- A crates.io trusted publisher for `jolars/polydoc`, the
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
