# Rust API extraction

Rust extraction uses Cargo and rustdoc to document the compiler-selected public
API, including reexports from private modules, macro-generated declarations,
generic signatures, traits, associated items, and implementations. Crate and
module documentation appear alongside item documentation. Hidden items stay out
of the public reference. Type and trait pages group their fields, variants,
methods, and implementations under stable anchors.

Declare one explicit library entry file:

```yaml
packages:
  - id: "eunoia"
    name: "Eunoia"
    slug: "rust"
    ecosystem: "rust"
    repository: "eunoia"
    path: "crates/eunoia"
    metadata-path: "Cargo.toml"
    targets:
      - id: "api"
        extractor: "rust"
        path: "src/lib.rs"
        role: "public-api"
```

Cargo supplies workspace-inherited metadata. Targets must match the library
entry declared by Cargo. Rust extraction currently uses the host target; binary
targets and cross-compilation are unsupported. Cargo's default features remain
enabled unless the target supplies `rust: { default-features: false }`.
Use `rust: { features: ["parallel"] }` to select additional features. Features
apply equally to API extraction and doctests; all features are never enabled
implicitly.

The documentation toolchain is `nightly-2026-09-28`, separate from Diplodocus's
stable compiler and the documented package's toolchain. `devenv shell` prepares
it automatically. Elsewhere, install that nightly with Cargo, rustc, and rustdoc,
and set `DIPLODOCUS_RUST_TOOLCHAIN` to its toolchain root. Diplodocus validates
the compiler identity and rustdoc JSON format before using them. It does not
install the toolchain. Prepare the package's Cargo.lock and cached dependencies
before extraction; all native Cargo commands use `--locked --offline`.

Both `check` and extraction invoke native compiler tooling, which may compile
dependencies and execute build scripts. `check` resolves the actual Rust API and
documentation links but does not run examples. `extract`, `build`, and `serve`
also compile and execute native doctests before publication. Bare fences and
Rust fences follow rustdoc semantics, including hidden setup lines, explicit
editions, `no_run`, `compile_fail`, `should_panic`, and `ignore` attributes.
Snapshots distinguish execution, compilation alone, expected errors, and
explicit skips. Native successful stdout may be absent from the recorded output.

Intra-doc links retain their labels and resolve to local pages or external crate
documentation. Authored semantic links can use the configured package ID and a
Rust path, such as `eunoia::eunoia::Fitter`, or a crate-relative name within that
package, such as `eunoia::Fitter`. Reexports share one canonical item identity.
Source links retain the original definition location.

Snapshots retain Rust declarations, resolved references, input fingerprints,
compiler and dependency evidence, and completed doctest outcomes. Generation
requires neither the checkout nor the Rust toolchain. Compilation artifacts may
be reused, but extraction and doctest outcomes are recomputed on each build.
Failures preserve the previous snapshot and site. The
[Rust extraction contract](https://github.com/jolars/diplodocus/blob/main/docs/ir/rust-extraction.md)
describes the portable representation and native input boundary.

