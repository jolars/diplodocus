# Installation

Diplodocus is under active development. Until its first crate release, build the
CLI from a source checkout with the pinned Rust toolchain and dependencies:

```console
git clone https://github.com/jolars/diplodocus.git
cd diplodocus
devenv shell
cargo build --locked --bin diplodocus
cargo run --locked -- --help
```

The `devenv` shell also provides Python, R, Jupyter kernels, and browser tools
used by the project examples and tests. A workspace that only contains Markdown
or static Python and Julia API extraction does not need those language runtimes.
R API extraction requires R on `PATH` and installed dependencies for package-wide
imports. Rd examples additionally require `pkgload`, `evaluate`, a loadable
checkout with prebuilt extensions, and the dependencies used by those examples.
Extraction loads that checkout and executes its examples. Generating a site from an exported snapshot needs none of
these runtimes. Executable QMD pages need the selected Jupyter
kernel already installed and discoverable. Diplodocus never installs a
workspace's dependencies or kernels.

After the crate is published, `cargo install diplodocus --locked` will install
the CLI from crates.io. Until then, run it with `cargo run --locked --` from
this checkout, or invoke the compiled binary at `target/debug/diplodocus`.

Continue with the [quick start](quick-start.md).
