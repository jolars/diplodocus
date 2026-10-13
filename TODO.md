# Project status and remaining work

Reviewed October 12, 2026. This page tracks work that remains before the first
release and the collaborator handoff. It replaces the old milestone-by-milestone
implementation log; Git history preserves that log and its completed checklists.
A checked implementation task is not, by itself, a release gate.

## What works today

  | Area               | Current implementation                                                                                                                                 | Evidence to start with                                                                                             |
  | ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
  | Workspace input    | Explicit repository, package, content, concept, relationship, and execution configuration; static Python, R, and Julia extraction; GFM and QMD parsing | `src/configuration.rs`, `src/extractors/`, `src/documents/`; `tests/acceptance_gate.rs`                            |
  | Authored execution | Explicitly authorized Python and R Jupyter pages, structured and validated output, bounded cleanup, and a page cache                                   | `src/execution/`; `tests/milestone_six.rs`, `tests/execution_cache.rs`                                             |
  | Portable handoff   | Workspace assembly, reference resolution, and a self-contained SQLite snapshot with canonical text export                                              | `src/assembly/`, `src/validation/`, `src/snapshots/`; `tests/snapshot_publication.rs`, `tests/monorepo_example.rs` |
  | Site               | Snapshot-backed routes, HTML, navigation, search, local assets, and atomic publication                                                                 | `src/site.rs`, `src/rendering/`; `tests/site_generation.rs`, `tests/browser/site.spec.ts`                          |
  | Commands           | `check`, `extract`, `generate`, `build`, and watched `serve`, including optional live reload                                                           | `src/commands/`; `tests/commands.rs`, `tests/cli.rs`                                                               |

These are implemented paths, not a claim that every acceptance and release check
below has passed on a clean checkout. The [architecture](DESIGN.md),
[collaborator code map](docs/development/handoff.md), [CLI
guide](docs/guide/cli.md), and [acceptance
matrix](tests/fixtures/acceptance/MATRIX.md) give their respective contracts.
The [three-language example](examples/monorepo/README.md) is the smallest
complete site.

The accepted [prepared-environment
contract](docs/decisions/0003-prepared-environment.md) changes the intended
extraction behavior. Runtime assistance and required API-example execution are
not yet implemented; the table above describes the existing paths.

## MVP acceptance criteria

These stable IDs connect the [case
registry](tests/fixtures/acceptance/CASES.json) and [acceptance
matrix](tests/fixtures/acceptance/MATRIX.md) to the release gate. Each criterion
still needs its complete scenario evidence before release.

- **MVP-01:** Explicit configuration for the full Python/R workspace.
- **MVP-02:** Python and R API extraction in caller-prepared package
  environments, with source analysis and runtime assistance as needed.
- **MVP-03:** In-process GFM and QMD parsing with visible unsupported syntax.
- **MVP-04:** Required execution of recognized API and authored examples, with
  structured results, expected-output verification, and explicit recorded skips.
- **MVP-05:** One safe site renderer with navigation, concepts, links, and
  search.
- **MVP-06:** Correct behavior and failure boundaries for all five commands.
- **MVP-07:** Deterministic, portable snapshots and site output.
- **MVP-08:** No implicit installation, dependency resolution, package builds,
  or network access by Diplodocus. Extraction authorizes package loading and
  example execution; `check` does not execute package code, and `generate`
  requires no package environment.
- **MVP-09:** Passing acceptance, quality, and end-to-end checks.
- **MVP-10:** Checked, previewed, and published self-documentation.
- **MVP-11:** Reproducible new-user and separate-stage workflows.

## Next handoff

The [code map](docs/development/handoff.md) describes the generator boundary.
The [monorepo integration test](tests/monorepo_example.rs) already copies a
snapshot, removes its source workspace, and compares the independently generated
site byte for byte with the original site.

- [ ] Produce a reviewed monorepo snapshot fixture from the real `extract`
  command, together with its canonical text export and example queries. Keep
  generated files out of the source fixture; the test should create them in
  a temporary directory. The [schema](docs/design/snapshot-schema.md)
  defines its wire format.

## Before the first release

### Product and acceptance

- [ ] Implement the [prepared-environment
  contract](docs/decisions/0003-prepared-environment.md): validate runtime
  and package prerequisites, load the supplied checkout, and distinguish
  source facts from runtime observations when reconciling API semantics.
  Keep source validation available through `check`.
- [ ] Execute recognized API examples and authored cells during extraction
  without a workspace opt-in. Define language-specific setup, session state,
  expected-output comparison, expected errors, illustrative syntax, and
  recorded conditional skips. Diagnose unsupported executable syntax and
  unavailable engines instead of leaving examples unchecked.
- [ ] Extend provenance, cache identities, and snapshot validation for runtime
  observations and API-example outcomes, outputs, and assets. Select any
  required schema and execution-policy version changes before publishing new
  records.
- [ ] Replace source-only acceptance packages with usable checkout environments,
  including compiled extensions. Keep parser-only fixtures for component
  tests. Update the case registry and matrix as implementation lands, and
  cover missing prerequisites, incorrect package identity, example errors
  and output mismatches, explicit skips, failure preservation, and
  runtime-free generation.
- [ ] Update configuration, language references, guides, examples, and CI
  environments alongside implementation. Remove obsolete execution opt-in
  and runtime-free extraction promises only when the new behavior is
  available.
- [ ] Audit the [acceptance matrix](tests/fixtures/acceptance/MATRIX.md) against
  the current implementation. Confirm routes, navigation and visibility,
  semantic links, search, source links, output safety, and assets with
  focused tests. Update expectations only after inspecting the product
  behavior.
- [ ] Verify command equivalence and failure boundaries across the complete
  acceptance workspace: separate `extract` plus `generate` versus `build`,
  generation from a relocated snapshot, and preservation of the last
  successful snapshot and site after failures.
- [ ] Compare two builds from identical declared inputs in different absolute
  paths. Require byte-identical site trees and logically identical snapshot
  records and assets; SQLite file bytes need not match.
- [ ] Audit portable records, diagnostics, HTML, and search data for local path
  leaks and nondeterministic metadata. Confirm that actual R parser
  observations and execution toolchain and kernel observations populate
  provenance, closing the old Milestone 3 subtask. Run source and command
  tests with network access denied where the declared environment permits
  it.
- [ ] Complete browser review at desktop and mobile sizes, including keyboard
  navigation, focus, headings, labels, contrast, links, and search. Use
  `site-capture` and inspect its screenshots and failure traces.

### Project documentation

- [x] Finish the dogfooded user guide: installation, supported GFM and QMD
  syntax, Python and R extraction limits, diagnostics, snapshot
  compatibility, and the security model for unsandboxed authored execution.
  Keep user guidance separate from design decisions and dated test reports.
- [x] Add and verify an in-tree project-site check covering local links,
  fragments, assets, and search. Confirm that a content-only workspace works
  without API extraction targets and that generated outputs are outside
  watched inputs.
- [x] Add the documentation publishing workflow. Build the project site on pull
  requests and `main`; deploy the Diplodocus-generated tree from version
  tags or an explicit manual dispatch to Cloudflare Workers at
  `diplodocus.cc`.

### Release

- [ ] Run the full Rust, browser, snapshot, execution, and documentation gates
  from a clean checkout in the declared `devenv` environment. Include
  `cargo fmt --all -- --check`,
  `cargo clippy --locked --all-targets --all-features -- -D warnings`,
  `cargo test --locked --all-targets`,
  `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps`, `site-capture`,
  and `cargo publish --locked --dry-run`.
- [ ] Verify that the Versionary release pull request, version tag, protected
  crate publishing, and documentation deployment trigger only as intended.
- [ ] Re-run the documented quick start from a clean checkout with declared
  tools installed. Verify the prepared package example, the acceptance site,
  and independent generation from an exported snapshot.

## R documentation gaps

Comparing eulerr with its [pkgdown site](https://jolars.github.io/eulerr/)
exposed these compatibility, content, and presentation gaps.

- [x] Resolve standard S3 generics from default R packages and package-wide
  imports without loading package code. Handle documented internal helpers
  without requiring public declarations, and keep internal grid hooks out of
  public navigation and search.
- [x] Support dataset topics, including bare object usage and the `docType`,
  `format`, and `source` sections. Use eulerr's `fruits`, `organisms`,
  `pain`, and `plants` datasets as acceptance cases.
- [x] Support Rd links with options, including local targets, package-qualified
  targets, and display labels; equations, tables, custom sections, named
  return-value entries, conditional content, and DOI links.
- [x] Present shared help topics with their complete S3 usage and argument
  listings, methods by class, and method navigation. Resolve help aliases
  shared across multiple declarations, including eulerr's diagram
  composition topic.
- [x] Execute Rd examples with results and figures as a required extraction
  step. Preserve explicitly illustrative `dontrun` regions, execute
  `donttest` regions, and record supported conditional skips under the
  prepared-environment contract.
- [x] Support R Markdown vignettes with knitr chunks, inline R, math, citations,
  and generated figures. Use eulerr's seven vignettes to check the complete
  article workflow.
- [x] Handle documented constants and deprecated help aliases, and preserve
  nested Rd list content. Verified `EULER_PANEL_PAD_PT`, `eulerr_deprecated`,
  and `eulerr_options` through full-package extraction and site generation.
- [x] Build package landing pages from README and DESCRIPTION content, retain
  package metadata through assembly, and expose NEWS, citation information,
  authors, licenses, and community links.
- [ ] Improve reference presentation with descriptive titles and URLs, source
  links, introductory prose before usage, topic summaries, syntax
  highlighting, and copy buttons. Remove duplicate signature blocks.
- [ ] Rank exact API names ahead of suffix matches in search, index readable
  documentation and aliases, and avoid duplicate results for shared topics.

## Datamonger dogfooding gaps

Use the sibling [Datamonger assessment](../datamonger/documentation/assessment.md)
and [site configuration](../datamonger/documentation/diplodocus.toml) to reproduce
these findings. Keep fixes covered by small, isolated fixtures in this repository
so regression tests do not require the sibling checkout.

- [x] Consolidate Python overloads into one documented API page and search
  result. Datamonger's [`fetch_data`](../datamonger/packages/python/src/datamonger/_api.py)
  now has one family page with its implementation prose and all three overload
  signatures. Individual overload references target stable signature anchors;
  navigation and search list the callable once.
- [x] Prefer public Python reexport names in titles, navigation, and search.
  Datamonger's root exports now use names such as `datamonger.fetch_data`.
  Implementation names and source locations remain in snapshots. Both names
  resolve to the same page. Use [the root exports](../datamonger/packages/python/src/datamonger/__init__.py)
  as the motivating case.
- [x] Recognize annotated Python type aliases such as
  `DatasetData: TypeAlias` in [Datamonger's value objects](../datamonger/packages/python/src/datamonger/_models.py).
  Snapshots retain `DatasetData` as a type alias with its union definition
  and semantic links from signatures. Imported and qualified `TypeAlias`
  markers from `typing` and `typing_extensions` are supported.
- [x] Resolve authored overrides before parsing automatically discovered R
  package documents. Datamonger's [community override](../datamonger/documentation/package-pages/r/community.md)
  replaces the root [contributor guide](../datamonger/CONTRIBUTING.md), and the
  unused guide no longer emits `LINE_PREFIX` warnings. Replaced documents retain
  source aliases without parsing their content; explicitly included documents
  still receive diagnostics. An isolated regression covers the guide's wrapped
  lists and nested code fence.
- [ ] Define how automatically discovered package documents link to repository
  files outside declared content. Datamonger's contributor guide initially
  failed on links to `spec/`, `TRUST.md`, and `OPERATIONS.md`. Consider repository
  URL fallback for existing, undeclared files without concealing missing local
  targets or weakening source boundaries.
- [x] Exercise runtime-assisted Julia extraction with the 15 exported exception
  types generated by `@define_error` in
  [errors.jl](../datamonger/packages/julia/src/errors.jl). Preserve the static
  `check` contract and distinguish runtime observations from source facts.
  Once complete extraction works, enable the Julia target and extend the nine
  shared concepts in Datamonger's configuration.
- [ ] Add an offline Datamonger example using
  [the shared conformance corpus](../datamonger/tests/conformance/README.md)
  to exercise retrieval, decoding, and rendered output in a prepared package
  environment. The current package has no Rd examples or R Markdown vignettes,
  so successful site builds do not verify example execution.
- [ ] Improve the motivating package's source documentation: add explanations
  for Python dataclass fields and `BUNDLED_REGISTRY`, and Rd topics or aliases
  for the eight undocumented R S3 methods listed in the assessment. Track these
  as Datamonger content fixes rather than extractor failures.

## Scope held for later

Incremental extraction and rendering; older snapshot migrations; historical
release assembly; additional language extractors; automatic repository
discovery; external extractor, renderer, or execution plugins; whole-document
notebooks; cell-level caching; arbitrary themes and Quarto compatibility; and a
hosted service are outside the first release. Browser live reload is already
implemented as an optional `serve --live-reload` feature.

- [ ] Add first-class interactive component embedding for authored pages.
  Diplodocus should provide authoring syntax for declaring a component and
  its inputs, manage mounting and asset loading, and preserve the complete
  bundle in portable snapshots so `generate` works without the original
  sources. External tools should compile components built with Svelte,
  TypeScript, WASM, or other browser technologies; Diplodocus should define
  the embedding contract rather than build its own frontend framework or
  bundler. Support JavaScript, CSS, WASM, workers, and related assets while
  preserving bundle paths. Use Eunoia's integrated diagram app and inline
  documentation examples as motivating cases, with a small browser runtime
  or adapters where useful.

Change this page when a release gate changes. Keep detailed validation evidence
in tests and CI results rather than adding another dated ledger.
