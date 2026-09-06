# Polydoc MVP Roadmap

This roadmap turns the initial product described in [DESIGN.md](DESIGN.md) into
an ordered implementation plan. The MVP is one reproducible documentation
snapshot for an explicitly configured workspace containing related Python and R
packages. It is not a general documentation platform.

## How to use this roadmap

- Complete the milestones in order. A milestone is complete only when its exit
  gate passes.
- Add a failing test or fixture before implementing each observable behavior.
- Keep the acceptance workspace as the source of truth for polyglot behavior and
  Polydoc's own site as the source of truth for authored-documentation behavior;
  use smaller fixtures only for focused error cases.
- Treat diagnostic text, generated HTML, and serialized IR as snapshot-tested
  output. Review intentional changes rather than updating snapshots blindly.
- Keep output deterministic by sorting filesystem discoveries and map-like data
  before assigning identities, resolving links, or rendering.
- Update `DESIGN.md` before implementing a change that alters the product
  boundary or contradicts an architectural principle.

## MVP completion criteria

The MVP is complete when all of the following are true:

- [ ] One `polydoc.toml` can describe local repositories, Python and R packages,
  extraction targets, authored content, package relationships, and
  conceptual API groups.
- [ ] Static extraction documents Python and R public APIs without importing a
  Python package or loading an R package.
- [ ] Polydoc renders authored pages and both API ecosystems through one HTML
  renderer with shared navigation, source links, semantic references,
  concept switchers, and workspace-wide search.
- [ ] `polydoc check`, `polydoc build`, and `polydoc serve` satisfy the command
  contract below.
- [ ] Repeated builds from identical declared inputs are byte-for-byte identical
  and contain no machine-specific checkout paths.
- [ ] Normal Polydoc operation neither installs dependencies nor accesses the
  network.
- [ ] The acceptance corpus passes formatting, linting, unit, golden,
  integration, link, and end-to-end tests.
- [ ] Polydoc builds, checks, previews, and publishes its own project
  documentation without another site generator.
- [ ] A new user can build and preview the acceptance site by following the
  checked-in documentation.

## MVP interface contract

### Commands

  | Command         | MVP behavior                                              |
  | --------------- | --------------------------------------------------------- |
  | `polydoc check` | Load, extract, and validate without producing a site.     |
  | `polydoc build` | Run the complete pipeline and render the static site.     |
  | `polydoc serve` | Build, serve locally, watch declared inputs, and rebuild. |

All commands accept `--config`; its default is `./polydoc.toml`. `build` and
`serve` accept `--output`, whose default is `./site`. `serve` also accepts
`--host` and `--port`, defaulting to `127.0.0.1` and `8000`.

Errors produce a nonzero exit status. Warnings remain visible but do not fail a
command. A failed watched rebuild must leave the last successful site available
and print the new diagnostics. Browser live reload is not part of the MVP.

### Content and output

- Polydoc Markdown is a safe GitHub-Flavored Markdown subset with fenced code,
  tables, admonitions, and Polydoc semantic references. Raw HTML is escaped.
- Python documentation consists of PEP 257 prose and NumPy-style structured
  sections.
- Project content is mounted at its configured path. Package documentation is
  rooted at `/packages/<slug>/`.
- Generated CSS, JavaScript, fonts, search data, and other runtime assets are
  local to the output tree; rendered pages do not depend on a CDN.
- Portable IR and rendered output use repository-relative source locations,
  never absolute checkout paths.

## Milestone 1: Establish the acceptance corpus

Build the representative workspace before choosing parser libraries or fixing
the IR. Keep it small enough to understand and broad enough to exercise the
MVP's differentiating behavior.

- [x] Create `tests/fixtures/acceptance/` with a documentation workspace and
  sibling `core`, `python`, and `r` repository roots.
- [x] Add a `polydoc.toml` that uses repository paths outside the configuration
  directory and package, target, content, relationship, and concept entries
  from `DESIGN.md`.
- [ ] Add a Python distribution with:
  - [ ] package metadata and a version;
  - [ ] public functions, classes, methods, properties, and constants;
  - [ ] an explicit `__all__`, package re-exports, and a dynamic export that
    must produce a diagnostic;
  - [ ] overloads and a corresponding callable family;
  - [ ] implementation files, maintained `.pyi` files, and a stub-only native
    extension module; and
  - [ ] NumPy-style parameters, returns, notes, references, and examples.
- [ ] Add an R package with:
  - [ ] `DESCRIPTION`, `NAMESPACE`, source files, a version, and dependencies;
  - [ ] exported functions, an S3 generic, and S3 methods;
  - [ ] parsed `Rd` aliases, usage, arguments, value, references, and examples;
    and
  - [ ] an unsupported or incomplete construct that must produce a diagnostic.
- [ ] Add project- and package-owned Markdown collections with nested pages,
  fenced code, a table, an admonition, a checked-in asset, package-qualified
  references, an unqualified reference, and an unsupported directive.
- [ ] Declare at least one equivalent concept and one analogous concept joining
  Python and R callable families.
- [ ] Represent public, internal, and hidden units, plus compatible,
  incompatible, and external package relationships in focused fixture
  variants.
- [ ] Write an acceptance matrix that maps every fixture construct to its
  expected IR, diagnostic, URL, navigation, link, concept, and search
  behavior.
- [ ] Use the Milestone 0 helpers to copy fixture workspaces into temporary
  directories so tests never modify checked-in inputs.

**Exit gate:** The corpus and acceptance matrix cover every MVP completion
criterion, each deliberately invalid variant has one documented expected failure
rather than several accidental failures, and all fixture tests run in the devenv
shell and GitHub Actions.

## Milestone 2: Spike static extraction

Use the acceptance corpus to discover what can be represented reliably. The
spikes choose implementation tools; they do not weaken the static-execution
contract.

- [ ] Compare viable Rust parsing and metadata libraries against every Python
  construct in the acceptance matrix.
- [ ] Verify that Python exports, re-exports, annotations, decorators,
  overloads, source spans, `.pyi` precedence, and docstrings can be obtained
  without importing the package.
- [ ] Compare viable R metadata, namespace, source, and `Rd` parsing approaches
  against every R construct in the matrix.
- [ ] If `Rscript` is required to parse `Rd`, define a checked-in, versioned,
  machine-readable helper protocol that calls parsing tools but never
  attaches, installs, or loads the documented package.
- [ ] Record how each extractor reports missing tools, unsupported syntax,
  dynamic exports, incomplete source locations, and information loss.
- [ ] Define each extractor's static mode, tool requirements, capabilities, and
  provenance fields.
- [ ] Record the selected approaches and rejected alternatives in
  `docs/decisions/0001-static-extraction.md`.
- [ ] Capture exploratory output as golden fixtures before replacing spike code
  with production extractors.

**Exit gate:** Every required Python and R construct has a selected extraction
path or an explicit diagnostic, and no selected path imports or loads documented
package code.

## Milestone 3: Implement configuration, diagnostics, and IR

Write focused failing tests for every validation rule and serialization shape
before implementing the model.

- [ ] Parse the `project`, `repository`, `package`, `content`, `concept`, and
  relationship configuration described in `DESIGN.md`.
- [ ] Apply documented defaults for package kind and visibility while requiring
  explicit repositories, packages, and extraction targets.
- [ ] Resolve repository paths relative to the configuration file, package paths
  relative to repositories, target and metadata paths relative to packages,
  and content paths relative to repositories.
- [ ] Reject missing roots, path traversal, and symlink escapes from each
  declared repository or package boundary.
- [ ] Diagnose duplicate repository, package, target, content, and concept IDs;
  duplicate package slugs; unknown owners; and unknown unqualified workspace
  relationship endpoints. Accept explicit external package coordinates.
- [ ] Define deterministic diagnostics with a stable code, severity, message,
  related entity, source path, and source span when available.
- [ ] Define a schema-versioned IR for repositories, packages, extraction
  targets, content collections, pages, items, signatures, documents,
  concepts, relationships, diagnostics, and provenance.
- [ ] Add typed Python and R item extensions instead of flattening
  language-specific semantics into generic fields.
- [ ] Represent signatures and documents as structured nodes rather than display
  strings or extractor-produced HTML.
- [ ] Define stable package-scoped item IDs that distinguish overloads,
  generics, methods, aliases, and other same-name entities while remaining
  independent of rendered URLs.
- [ ] Normalize source locations to repository IDs and forward-slash-separated,
  repository-relative paths.
- [ ] Collect repository revisions, dirty states, declared-input fingerprints,
  extractor versions, toolchain versions, and extraction modes without
  writing machine-specific paths into portable data.
- [ ] Parse the MVP Polydoc Markdown subset into document IR, including semantic
  references and visible placeholders for unsupported directives.
- [ ] Use ordered collections or explicit sorting wherever filesystem or hash
  iteration could affect serialized IR, diagnostics, or output.

**Exit gate:** Configuration and document fixtures have stable golden IR;
invalid path and identity cases yield stable diagnostics; serializing the same
model twice produces identical bytes and no absolute paths.

## Milestone 4: Implement the Python extractor

- [ ] Add golden tests for each Python acceptance case before implementing it.
- [ ] Read package name, version, and dependency metadata from `pyproject.toml`
  without invoking a build backend.
- [ ] Parse maintained `.py` and `.pyi` sources statically and retain source
  spans.
- [ ] Treat a statically resolvable `__all__` as authoritative; diagnose dynamic
  export computation that cannot be resolved safely.
- [ ] Without `__all__`, expose public definitions and explicit public imports
  from the documented module; exclude underscore-prefixed names by default.
- [ ] Resolve package and module re-exports without assigning a second identity
  to the same public item.
- [ ] Prefer a maintained `.pyi` surface over the corresponding implementation
  surface and support stub-only native extension modules.
- [ ] Extract functions, classes, methods, properties, constants, parameters,
  annotations, defaults, return types, decorators, and async state.
- [ ] Preserve individual overloads as addressable items and connect them to a
  public callable family.
- [ ] Parse PEP 257 prose and NumPy-style Parameters, Returns, Raises, Notes,
  References, and Examples sections into document IR.
- [ ] Emit stable diagnostics for syntax errors, unresolved re-exports,
  conflicting stubs, unsupported decorators, and incomplete docstring
  syntax.
- [ ] Record extractor capabilities, version, static mode, and required tools in
  provenance.

**Exit gate:** Python extraction matches the reviewed golden IR for every
acceptance case, remains unchanged when imports would have side effects, and
reports every unsupported case without silently dropping public information.

## Milestone 5: Implement the R extractor

- [ ] Add golden tests for each R acceptance case before implementing it.
- [ ] Read package name, version, title, and dependency constraints from
  `DESCRIPTION` without installing or loading the package.
- [ ] Parse `NAMESPACE` exports, S3 registrations, imports, and relevant method
  declarations.
- [ ] Parse maintained R source sufficiently to identify documented functions,
  formals, generics, methods, aliases, and available source spans.
- [ ] Parse checked-in `Rd` without loading package code and translate names,
  aliases, usage, arguments, value, description, details, references,
  examples, and supported markup into document IR.
- [ ] Preserve a generic and each S3 method as addressable items and connect
  them through a callable family.
- [ ] Reconcile namespace exports, source definitions, and `Rd` aliases without
  duplicating one public entity.
- [ ] Emit stable diagnostics for malformed metadata, missing documented
  aliases, unsupported namespace directives, unsupported `Rd`, and missing
  external tools.
- [ ] Record extractor capabilities, version, static mode, R version, and helper
  protocol version in provenance.

**Exit gate:** R extraction matches the reviewed golden IR for every acceptance
case, works without attaching the package, and makes unsupported or incomplete
semantic information visible through diagnostics.

## Milestone 6: Merge, resolve, validate, and build the site model

- [ ] Add failing tests for fragment conflicts, references, relationships,
  concepts, routes, and visibility before implementing each behavior.
- [ ] Merge all extraction-target fragments for a package deterministically and
  diagnose duplicate or conflicting identities.
- [ ] Resolve package-qualified references such as
  ``[`pyfoo::foo.FooModel.fit`]`` directly against semantic identities.
- [ ] Resolve unqualified references first in the owning package and then in the
  workspace only when the result is unique; diagnose ambiguity and absence.
- [ ] Resolve concept authoring names to item or callable-family IDs and retain
  the distinction between equivalent, analogous, and related concepts.
- [ ] Validate package relationships and version constraints when both endpoints
  are present; report a known mismatch as an error and an indeterminate
  constraint as a warning.
- [ ] Retain external relationship coordinates as provenance without treating a
  missing external source repository as an error.
- [ ] Assign stable routes independently of semantic IDs and diagnose route or
  mount collisions before rendering.
- [ ] Build project navigation from content collections and public packages,
  then build package navigation by ecosystem-aware item category.
- [ ] Keep internal items linkable and searchable without placing their package
  in the project switcher; keep hidden items linkable but absent from
  navigation and search.
- [ ] Construct renderer-ready page, breadcrumb, source-link, navigation,
  concept-switcher, and search-entry models without embedding parser logic.
- [ ] Wire `polydoc check` through configuration, extraction, merging, reference
  resolution, and validation without creating the output directory.

**Exit gate:** `polydoc check` succeeds for the valid acceptance workspace,
fails with the expected diagnostics for every invalid variant, writes no site,
and produces the same ordered diagnostics on repeated runs.

## Milestone 7: Render the coherent site and search index

- [ ] Snapshot the intended HTML for representative project, content, package,
  category, item, and concept pages before completing their templates.
- [ ] Render all pages from the site model through one escaped HTML and asset
  pipeline; do not accept extractor-produced HTML.
- [ ] Add a project landing page, mounted authored pages, package overviews,
  category listings, item pages, and concept pages.
- [ ] Render structured Python and R signatures with ecosystem-appropriate
  syntax and layouts in the same visual system.
- [ ] Add project and package navigation, package switching, breadcrumbs,
  ecosystem labels, active states, and source links pinned to recorded
  revisions when available.
- [ ] Show "Same API in" for equivalent concepts and "Related API" for analogous
  or related concepts on both member and concept pages.
- [ ] Render supported document blocks, semantic links, checked-in assets,
  syntax-highlighted code, and visible placeholders for unsupported content.
- [ ] Generate a deterministic browser-side search index covering authored
  pages, packages, modules, types, functions, methods, signatures, and
  documentation text.
- [ ] Identify each search result by package and ecosystem and exclude hidden
  items.
- [ ] Bundle search behavior and all styling locally with usable keyboard,
  focus, contrast, narrow-screen, and no-JavaScript fallbacks.
- [ ] Generate site-local links independently of the hosting prefix so the same
  output works at an apex domain or beneath a GitHub Pages repository path.

**Exit gate:** Reviewed snapshots cover both ecosystems and authored content;
all generated internal links resolve; search returns the expected cross-package
results; and no rendered page requires a network resource.

## Milestone 8: Complete `build` and `serve`

- [ ] Wire `polydoc build` through the same checked pipeline and render only
  after error-free validation.
- [ ] Render into a temporary sibling directory and replace the configured
  output only after a successful build so failures cannot leave a partial
  site.
- [ ] Reject an output path that overlaps the configuration file or any declared
  repository input.
- [ ] Write schema-versioned snapshot provenance into the output without
  timestamps or absolute local paths.
- [ ] Make CLI diagnostics concise by default and sufficiently detailed to find
  the responsible configuration or source location.
- [ ] Make `polydoc serve` perform an initial build, bind only to its configured
  local address, and serve the successful output tree.
- [ ] Watch the configuration file and declared extraction, metadata, content,
  and asset inputs; ignore the output directory and unrelated repository
  files.
- [ ] Debounce related filesystem events into one rebuild and keep the previous
  successful output available when validation or rendering fails.
- [ ] Handle address conflicts, deleted inputs, changed workspace configuration,
  missing tools, and graceful process termination with stable diagnostics.
- [ ] Add integration tests for flag precedence, exit status, output
  replacement, HTTP serving, watched rebuilds, ignored changes, and recovery
  after a failed rebuild.

**Exit gate:** All three commands satisfy the interface contract in temporary
multi-repository workspaces; `serve` observes a source edit and exposes the new
page without a restart while preserving the last good site after an error.

## Milestone 9: Dogfood Polydoc for its own documentation

Use Polydoc---not another static-site generator---to build the documentation
users read about Polydoc. This self-documentation workspace exercises authored
content and project navigation; documenting the Rust API remains deferred until
a Rust extractor exists.

- [ ] Add a root `polydoc.toml` that declares this checkout as a repository and
  mounts project-owned content from `docs/`.
- [ ] Support and test a content-only workspace with no API extraction targets
  so the self-documentation configuration does not pretend that Polydoc has
  a Python or R public API.
- [ ] Make `docs/` the canonical source for the project overview, installation,
  quick start, workspace configuration, CLI, Polydoc Markdown, Python and R
  support, diagnostics, reproducibility, and the static-execution security
  model.
- [ ] Link design and contributor material where useful instead of copying
  internal rationale into user documentation.
- [ ] Use the supported Markdown features, navigation, checked-in assets, source
  links, and search in the real site so regressions affect the project
  before they affect downstream users.
- [ ] Add tests that run the in-tree binary against the root `polydoc.toml`,
  snapshot representative pages and the search index, and validate every
  local link and asset.
- [ ] Ensure the configured output directory is ignored and excluded from
  declared inputs so self-documentation builds cannot recurse into
  themselves.
- [ ] Use `polydoc serve` as the documented local preview workflow for changes
  under `docs/`.
- [ ] Add `.github/workflows/docs.yml`, modeled on Basin's website workflow, to
  build and validate the Polydoc site on pull requests, `main`, version
  tags, and manual dispatches.
- [ ] Upload and deploy only the Polydoc-generated output through GitHub Pages;
  use the `github-pages` environment and the minimal `pages: write` and
  `id-token: write` permissions in the deployment job.
- [ ] Build the site for pull requests and `main`, but deploy only from `v*`
  version tags or an explicit manual dispatch, following Basin's separation
  of build verification from publication.
- [ ] Add `.nojekyll` as a deployment artifact without placing generated files
  in source control.
- [ ] Test assets, navigation, and search beneath the repository Pages path
  `/polydoc/`; do not hard-code a deployment origin or assume an apex
  domain.

**Exit gate:** A clean checkout builds the complete project site with the
in-tree `polydoc` binary, the result passes link and asset checks, local preview
uses `polydoc serve`, and the GitHub Pages workflow deploys exactly that
generated tree without invoking another documentation generator.

## Milestone 10: Harden and release the MVP

- [ ] Run the complete acceptance workspace through `check`, `build`, and
  `serve` in end-to-end tests.
- [ ] Build the same declared inputs twice in different absolute directories and
  compare every output path and byte.
- [ ] Scan portable IR, provenance, HTML, search data, and diagnostics for
  leaked absolute paths and nondeterministic metadata.
- [ ] Run normal command tests with network access disabled and fixtures whose
  package imports or load hooks would fail if executed.
- [ ] Validate every internal HTML link, fragment, source-link shape, asset URL,
  navigation target, concept target, and indexed result.
- [ ] Exercise missing tools, malformed configuration, malformed sources,
  unsupported constructs, version mismatches, ambiguous references, and
  output write failures.
- [ ] Review generated pages at narrow and wide viewport sizes and verify
  keyboard access, focus indication, heading order, labels, and color
  contrast.
- [ ] Document installation, workspace configuration, command behavior,
  diagnostics, the supported Python/R surface, the Markdown subset, and the
  static-execution security model in the dogfooded project site.
- [ ] Add a quick start that builds and previews the acceptance site from a
  clean checkout with declared tools already installed.
- [ ] Build the dogfooded project site twice in different absolute checkout
  paths and compare every output path and byte.
- [ ] Complete the MVP's Versionary release pull request and verify that its
  `v*` tag triggers the protected crate-publishing and documentation
  workflows; ordinary branch builds must never publish.
- [ ] Run the release gate:
  - [ ] `cargo fmt --all -- --check`;
  - [ ] `cargo clippy --all-targets --all-features -- -D warnings`;
  - [ ] `cargo test --all-features`;
  - [ ] `cargo doc --no-deps` with `RUSTDOCFLAGS=-D warnings`;
  - [ ] `cargo publish --locked --dry-run`;
  - [ ] `polydoc check` on the acceptance workspace;
  - [ ] two byte-identical acceptance builds; and
  - [ ] a byte-identical dogfood build plus its link and asset checker.

**Exit gate:** Every MVP completion criterion at the top of this file is
checked, the release gate passes from a clean checkout, and the documented quick
start reproduces both the acceptance site and Polydoc's project site without
network access during Polydoc execution.

## Explicitly deferred until after the MVP

- Import-based Python introspection and load-based R introspection.
- Extraction caches and incremental extraction beyond `serve` rebuilding the
  current snapshot.
- Browser live reload.
- Historical snapshot assembly and version switching.
- Rust, Julia, TypeScript, C, or other public-API extractors; until a Rust
  extractor exists, Polydoc's dogfooded site documents its authored project and
  CLI material rather than generating Rust API reference pages.
- Automatic repository, package, or ecosystem discovery.
- A stable external extractor or renderer plugin API.
- Executed examples, notebooks, or authored code cells.
- Sphinx, MyST, pkgdown, Documenter.jl, R Markdown, or arbitrary theme
  compatibility.
- Additional content adapters and trusted raw HTML.
- The `polydoc init` and `polydoc extract` commands.
- A hosted documentation service operated by Polydoc, repository management,
  package installation, dependency resolution, or implicit version-control
  operations.
