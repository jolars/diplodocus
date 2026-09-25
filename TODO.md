# Diplodocus MVP Roadmap

This roadmap turns the initial product described in [DESIGN.md](DESIGN.md) into
an ordered implementation plan. The MVP is one reproducible documentation
snapshot for an explicitly configured workspace containing related Python and R
packages. Extraction publishes a portable SQLite database; generation reads it
to produce the static site. `build` runs both stages.

The next contributor handoff is a documented snapshot schema, a working
`extract` command, and a representative R/Python database with a canonical text
export. The non-executing portion of Milestone 7 can deliver this handoff before
Milestone 6 is complete, allowing frontend work against the snapshot to begin.
The full MVP still includes authorized authored execution.

## How to use this roadmap

- Follow milestone dependencies. The static snapshot handoff in Milestone 7
  may precede Milestone 6, and generation work in Milestone 8 may begin against
  that artifact. A milestone is complete only when its full exit gate passes.
- Add a failing test or fixture before implementing each observable behavior.
- Keep the acceptance workspace as the source of truth for polyglot behavior and
  Diplodocus's own site as the source of truth for authored-documentation
  behavior; use smaller fixtures only for focused error cases.
- Treat diagnostic text, generated HTML, and serialized IR as snapshot-tested
  output. Review intentional changes rather than updating snapshots blindly.
- Keep output deterministic by sorting filesystem discoveries and map-like data
  before assigning identities, resolving links, or rendering.
- Update `DESIGN.md` before implementing a change that alters the product
  boundary or contradicts an architectural principle.

## MVP completion criteria

The MVP is complete when all of the following are true:

- [ ] **MVP-01:** One `diplodocus.toml` can describe local repositories,
  Python and R packages, extraction targets, authored content profiles and
  execution, package relationships, and conceptual API groups.
- [ ] **MVP-02:** Rust-native static extraction documents Python and R public
  APIs without starting a language runtime, importing a Python package, or
  loading an R package.
- [ ] **MVP-03:** Diplodocus parses authored `.md` through its supported GFM
  profile and authored `.qmd` through its supported Quarto profile using
  `panache-parser` in-process, with visible diagnostics for unsupported
  syntax.
- [ ] **MVP-04:** Explicitly configured QMD collections execute Python and R
  code cells through installed Jupyter kernels and retain streams, errors,
  Markdown, and figure output as structured document IR.
- [ ] **MVP-05:** Diplodocus renders authored pages and both API ecosystems
  through one HTML renderer with safe code-cell output, shared navigation,
  source links, semantic references, concept switchers, and workspace-wide
  search.
- [ ] **MVP-06:** `check`, `extract`, `generate`, `build`, and `serve` satisfy
  the command contract below. Extraction publishes a versioned, self-contained
  SQLite snapshot; generation needs only that snapshot and the generator.
- [ ] **MVP-07:** Repeated builds of the deterministic acceptance cells from
  identical declared sources, environments, kernels, and toolchains are
  byte-for-byte identical in site output and logically identical in snapshot
  records and asset contents. Neither contains machine-specific checkout paths;
  SQLite file bytes need not match.
- [ ] **MVP-08:** Diplodocus neither installs dependencies nor performs
  implicit network access; authored execution is configuration-authorized and
  documented as arbitrary, unsandboxed code execution.
- [ ] **MVP-09:** The acceptance corpus passes formatting, linting, unit,
  golden, integration, link, and end-to-end tests.
- [ ] **MVP-10:** Diplodocus builds, checks, previews, and publishes its own
  project documentation without another site generator.
- [ ] **MVP-11:** A new user can build and preview the acceptance site, or
  extract a snapshot and generate the site separately, by following the
  checked-in documentation.

## MVP interface contract

### Commands

| Command | MVP behavior |
| --- | --- |
| `diplodocus check` | Load, parse, extract, and validate without executing cells or publishing a snapshot or site. |
| `diplodocus extract` | Parse and extract, run authorized execution, resolve references, validate, collect assets, and publish a SQLite snapshot. |
| `diplodocus generate` | Read and validate a snapshot, then build the site model and render the static site. |
| `diplodocus build` | Run extraction followed by generation with the same behavior as the separate commands. |
| `diplodocus serve` | Build, serve, watch declared inputs, and rebuild safely. |

`check`, `extract`, `build`, and `serve` accept `--config`, which defaults to
`./diplodocus.toml`. `extract --output` selects the snapshot path; its default is
`.diplodocus/documentation.sqlite` relative to the configuration directory.
`build` and `serve` use that default snapshot location.

`generate` requires an explicit `--input` snapshot. It uses recorded presentation
defaults and explicit presentation overrides without reading source checkouts,
workspace configuration, or execution caches, and never starts a language
runtime. `generate`, `build`, and `serve` accept `--output`, defaulting to
`./site`. `serve` also accepts `--host` and `--port`, defaulting to `127.0.0.1`
and `8000`.

Errors produce a nonzero exit status. Warnings remain visible but do not fail a
command. A failed extraction leaves the previous snapshot intact; `build` and
`serve` report the failure without generating from that stale snapshot. A failed
generation leaves the last successful site intact and preserves the newly
extracted snapshot for another attempt. Watched rebuilds print new diagnostics
while keeping the last successful site available. Browser live reload is not
part of the MVP.

`check` shares extraction and validation components but neither executes cells
nor publishes output. It cannot validate references introduced only by execution
results that do not yet exist.

### Content and output

- `.md` content uses Diplodocus's safe GFM profile; `.qmd` content uses its
  documented Quarto profile with executable fences, hashpipe options, callouts,
  and Diplodocus semantic references. Raw source HTML is escaped.
- Authored execution is disabled by default. An executing QMD collection names
  one installed Jupyter kernel, and each page uses one session with cells run in
  source order.
- Stream output is escaped; Markdown-valued output is parsed with execution
  disabled; figures become local content-addressed assets; and kernel HTML must
  cross an explicit sanitizer boundary before rendering.
- Python documentation consists of PEP 257 prose and NumPy-style structured
  sections.
- Project content is mounted at its configured path. Package documentation is
  rooted at `/packages/<slug>/`.
- Generated CSS, JavaScript, fonts, search data, and other runtime assets are
  local to the output tree; rendered pages do not depend on a CDN.
- A snapshot contains semantic records, recorded execution outputs, local
  content asset bytes, default presentation settings, provenance, schema and
  producer versions, stable IDs, and content fingerprints. The generator
  supplies the built-in theme; external hyperlinks remain unfetched links.
- Portable IR and rendered output use repository-relative source locations,
  never absolute checkout paths.

## Milestone 1: Establish the acceptance corpus

Build the representative workspace before choosing parser libraries or fixing
the IR. Keep it small enough to understand and broad enough to exercise the
MVP's differentiating behavior.

- [x] Create `tests/fixtures/acceptance/` with a documentation workspace and
  sibling `core`, `python`, and `r` repository roots.
- [x] Add a `diplodocus.toml` that uses repository paths outside the
  configuration directory and package, target, content, relationship, and
  concept entries from `DESIGN.md`.
- [x] Update the acceptance configuration with explicit GFM and QMD collections,
  `never` and `execute` modes, Python and R kernel names, and declared
  environment inputs.
- [x] Add a Python distribution with:
  - [x] package metadata and a version;
  - [x] public functions, classes, methods, properties, and constants;
  - [x] an explicit `__all__`, package re-exports, and a dynamic export that
    must produce a diagnostic;
  - [x] overloads and a corresponding callable family;
  - [x] implementation files, maintained `.pyi` files, and a stub-only native
    extension module; and
  - [x] NumPy-style parameters, returns, notes, references, and examples.
- [x] Add an R package with:
  - [x] `DESCRIPTION`, `NAMESPACE`, source files, a version, and dependencies;
  - [x] exported functions, an S3 generic, and S3 methods;
  - [x] parsed `Rd` aliases, usage, arguments, value, references, and examples;
    and
  - [x] an unsupported or incomplete construct that must produce a diagnostic.
- [x] Add project- and package-owned GFM and QMD collections with nested pages,
  display code, a table, a callout, a checked-in asset, package-qualified
  references, an unqualified reference, and an unsupported directive.
- [x] Add executable Python and R QMD pages with sequential stateful cells,
  hashpipe options, labels, stdout and stderr, Markdown-valued output, a
  figure, and a controlled error.
- [x] Add focused variants proving that GFM fences are display-only, QMD
  execution defaults to `never`, document metadata cannot authorize
  execution, and generated Markdown cannot introduce an executable cell.
- [x] Add output-safety variants containing Markdown-looking stdout, unsafe
  kernel HTML, and an asset path that attempts to escape its declared
  boundary.
- [x] Declare at least one equivalent concept and one analogous concept joining
  Python and R callable families.
- [x] Represent public, internal, and hidden units, plus compatible,
  incompatible, and external package relationships in focused fixture
  variants.
- [x] Write an acceptance matrix that maps every fixture construct to its
  expected IR, diagnostic, execution behavior, provenance, URL, navigation,
  link, concept, and search behavior.
- [x] Use the Milestone 0 helpers to copy fixture workspaces into temporary
  directories so tests never modify checked-in inputs.

**Exit gate:** The corpus and acceptance matrix cover every MVP completion
criterion, each deliberately invalid variant has one documented expected failure
rather than several accidental failures, and all fixture tests run in the devenv
shell and GitHub Actions.

Corpus evidence is tracked in the
[case and scenario registry](tests/fixtures/acceptance/CASES.json) and
[acceptance matrix](tests/fixtures/acceptance/MATRIX.md). At this gate,
coverage means concrete inputs and expected outcomes for every criterion;
fixture isolation and available parser/kernel checks run now. Full command
behavior remains assigned to its implementation milestone. The project-site
seed is part of the corpus, not a claim that Milestone 10 is complete.

## Milestone 2: Spike extraction, parsing, and execution

Use the acceptance corpus to discover what can be represented reliably. The
spikes choose implementation tools and establish the boundary between static API
extraction and explicitly authorized authored execution.

- [x] Compare viable Rust-native parsing and metadata libraries against every
  Python construct in the acceptance matrix.
- [x] Verify that Python exports, re-exports, annotations, decorators,
  overloads, source spans, `.pyi` precedence, and docstrings can be obtained
  without importing the package.
- [x] Compare viable Rust-native R metadata, namespace, source, and `Rd` parsing
  approaches against every R construct in the matrix.
- [x] Verify that `DESCRIPTION`, `NAMESPACE`, maintained R source, and
  checked-in `Rd` can be parsed in-process without `Rscript` or package
  loading.
- [x] Record how each extractor reports malformed metadata, unsupported syntax,
  dynamic constructs, incomplete source locations, and information loss.
- [x] Define each extractor's parser versions, static mode, capabilities, and
  provenance fields.
- [x] Pin `panache-parser` as the in-process reader and verify its GFM and
  Quarto flavors against every authored-content construct in the acceptance
  matrix.
- [x] Verify that Panache's typed syntax API exposes semantic block and inline
  traversal, unsupported nodes, embedded-YAML errors, source ranges, and QMD
  cell source and options without using its Pandoc projectors. Land the
  required consumer-facing API changes in Panache where the current surface
  is insufficient.
- [x] Compare the current `jupyter-zmq-client` and `jupyter-protocol` crates
  against the execution corpus: kernel discovery and startup, ordered cell
  execution, stream and error messages, MIME bundles, display updates,
  timeout, interruption, and shutdown.
- [x] Verify Python and R kernels in the declared devenv and CI environments
  without starting a Jupyter server or installing anything during the test.
- [x] Define the supported QMD metadata and cell-option subset, MIME preference
  order, HTML sanitization boundary, execution failure policy, toolchain
  requirements, and execution provenance fields. See [the authored-execution
  contract](docs/spikes/authored-execution-contract.md).
- [x] Define a deterministic page-level execution-cache key and artifact format
  covering source, normalized options, engine and kernel identity, relevant
  toolchain versions, and declared environment inputs. See [the page
  execution-cache contract](docs/spikes/page-execution-cache.md).
- [x] Record the selected approaches and rejected alternatives in
  `docs/decisions/0001-static-extraction.md`.
- [x] Record the authored-format and execution decisions, including rejected Q2,
  Pandoc-projector, temporary-Markdown, and direct-HTML boundaries, in
  `docs/decisions/0002-authored-content.md`.
- [x] Capture exploratory output as golden fixtures before replacing spike code
  with production extractors. See [the golden-fixture inventory and capture
  guide](docs/spikes/golden-fixtures.md).

**Exit gate:** Every required Python and R API construct has a selected
Rust-native static extraction path or an explicit diagnostic; every authored
construct has a Panache-to-IR path; Python and R kernels produce the required
structured outputs; and no static extractor starts a language runtime, imports a
package, or loads documented package code.

## Milestone 3: Implement configuration, diagnostics, and IR

Write focused failing tests for every validation rule and serialization shape
before implementing the model.

- [x] Parse the `project`, `repository`, `package`, `content`, `concept`, and
  relationship configuration described in `DESIGN.md`.
- [x] Require each content collection to select `gfm` or `qmd`; default
  execution to `mode = "never"`; and validate the `execute` mode, Jupyter
  engine, kernel, and declared environment inputs as one coherent unit.
- [x] Reject execution for GFM collections and reject any document metadata that
  attempts to broaden the collection's configured execution authority.
- [x] Apply documented defaults for package kind and visibility while requiring
  explicit repositories, packages, and extraction targets.
- [x] Resolve repository paths relative to the configuration file, package paths
  relative to repositories, target and metadata paths relative to packages,
  and content and declared-environment paths relative to repositories.
- [x] Reject missing roots, path traversal, and symlink escapes from each
  declared repository or package boundary.
- [x] Diagnose duplicate repository, package, target, content, and concept IDs;
  duplicate package slugs; unknown owners; and unknown unqualified workspace
  relationship endpoints. Accept explicit external package coordinates.
- [x] Define deterministic diagnostics with a stable code, severity, message,
  related entity, source path, and source span when available.
- [x] Define a schema-versioned IR for repositories, packages, extraction
  targets, content collections, pages, items, signatures, documents, code
  cells, cell outputs, output representations, concepts, relationships,
  diagnostics, and provenance.
- [x] Add typed Python and R item extensions instead of flattening
  language-specific semantics into generic fields.
- [x] Represent signatures and documents as structured nodes rather than display
  strings or extractor-produced HTML.
- [x] Define stable package-scoped item IDs that distinguish overloads,
  generics, methods, aliases, and other same-name entities while remaining
  independent of rendered URLs.
- [x] Normalize source locations to repository IDs and forward-slash-separated,
  repository-relative paths.
- [ ] Collect repository revisions, dirty states, declared-input fingerprints,
  extractor, parser, and Panache versions, execution toolchain and kernel
  versions, extraction and execution modes, and declared environment
  fingerprints without writing machine-specific paths into portable data.
  - [x] Collect static repository revision and dirty-or-unknown observations,
    declared-input and environment fingerprints, and built-in tool versions.
  - [x] Provide typed producer interfaces for extraction and execution evidence.
  - [x] Wire actual Python extractor/parser observations in Milestone 4.
  - [ ] Wire actual R extractor/parser observations in Milestone 5 and
    execution toolchain/kernel observations in Milestone 6.
- [x] Translate the supported GFM and QMD profiles from Panache's typed syntax
  views into document IR, including semantic references, code cells, source
  ranges, and visible placeholders for unsupported constructs.
- [x] Represent stream, error, display, Markdown-fragment, sanitized-HTML, and
  asset outputs as typed nodes; never place extractor-, parser-, or
  engine-produced HTML directly in a document.
- [x] Parse Markdown-valued cell output as an isolated fragment with execution
  disabled and provenance pointing to the producing cell.
- [x] Use ordered collections or explicit sorting wherever filesystem or hash
  iteration could affect serialized IR, diagnostics, or output.

**Exit gate:** Configuration and GFM/QMD document fixtures have stable golden
IR; execution authority, unsupported syntax, invalid paths, and identity cases
yield stable diagnostics; serializing the same model twice produces identical
bytes and no absolute paths.

The [integration tests](tests/milestone_three.rs) lock the acceptance
configuration and static evidence in a reviewed golden, check authored documents
and diagnostics across relocated workspaces, and reverse declared-input order.
Existing authored-document goldens cover the supported syntax trees. The
[item identity contract](docs/ir/item-identity.md) defines canonical identity and
alias handling; source/stub and export reconciliation remains extractor work.
Live producer provenance and HTML sanitization remain in Milestones 4–6.

## Milestone 4: Implement the Python extractor

- [x] Add golden tests for each Python acceptance case before implementing it.
- [x] Read package name, version, and dependency metadata from `pyproject.toml`
  without invoking a build backend.
- [x] Parse maintained `.py` and `.pyi` sources statically and retain source
  spans.
- [x] Treat a statically resolvable `__all__` as authoritative; diagnose dynamic
  export computation that cannot be resolved safely.
- [x] Without `__all__`, expose public definitions and explicit public imports
  from the documented module; exclude underscore-prefixed names by default.
- [x] Resolve package and module re-exports without assigning a second identity
  to the same public item.
- [x] Prefer a maintained `.pyi` surface over the corresponding implementation
  surface and support stub-only native extension modules.
- [x] Extract functions, classes, methods, properties, constants, parameters,
  annotations, defaults, return types, decorators, and async state.
- [x] Preserve individual overloads as addressable items and connect them to a
  public callable family.
- [x] Parse PEP 257 prose and NumPy-style Parameters, Returns, Raises, Notes,
  References, and Examples sections into document IR.
- [x] Emit stable diagnostics for syntax errors, unresolved re-exports,
  conflicting stubs, unsupported decorators, and incomplete docstring
  syntax.
- [x] Record extractor capabilities, extractor and parser versions, and static
  mode in provenance.

**Exit gate:** Python extraction matches the reviewed golden IR for every
acceptance case, remains unchanged when imports would have side effects, and
reports every unsupported case without silently dropping public information.

The [integrated Python tests](tests/milestone_four.rs) lock the complete
acceptance fragment, compare relocated workspaces, verify the sole dynamic-export
warning, and exercise inputs that must never be imported or built. The
[Python extraction contract](docs/ir/python-extraction.md) documents the library
entry point, supported surface, and source-attribution rules.

## Milestone 5: Implement the R extractor

- [x] Add golden tests for each R acceptance case before implementing it.
- [x] Read package name, version, title, and dependency constraints from
  `DESCRIPTION` without installing or loading the package.
- [x] Parse `NAMESPACE` exports, S3 registrations, imports, and relevant method
  declarations.
- [x] Parse maintained R source sufficiently to identify documented functions,
  formals, generics, methods, aliases, and available source spans.
- [x] Parse checked-in `Rd` without loading package code and translate names,
  aliases, usage, arguments, value, description, details, references,
  examples, and supported markup into document IR.
- [x] Preserve a generic and each S3 method as addressable items and connect
  them through a callable family.
- [x] Reconcile namespace exports, source definitions, and `Rd` aliases without
  duplicating one public entity.
- [x] Emit stable diagnostics for malformed metadata, missing documented
  aliases, unsupported namespace directives, unsupported `Rd`, incomplete
  source locations, and information loss.
- [x] Record extractor capabilities, extractor and parser versions, and static
  mode in provenance.

**Exit gate:** R extraction matches the reviewed golden IR for every acceptance
case, works without starting R or attaching or loading the package, and makes
unsupported or incomplete semantic information visible through diagnostics.

The [integrated R tests](tests/r_extraction.rs) lock the seven acceptance items,
shared Rd documentation, four baseline location warnings, and the additional
dynamic-Rd warning. They compare relocated workspaces and verify extraction
with no runtime on `PATH`. The [R extraction contract](docs/ir/r-extraction.md)
documents the library entry point, supported subset, and file-level Rd
attribution. CLI integration and workspace merging remain later milestones.

## Milestone 6: Implement authored code execution

Keep execution separate from parsing and rendering. Tests should use the
smallest deterministic kernels and cells that exercise the Jupyter protocol and
Diplodocus's document transformation.

Follow the [remaining-work plan](docs/design/execution-remaining-plan.md) for
dependencies, ownership, and acceptance. Identity and output safety precede the
parallel engine and cache work. The execution-core gate can pass before the
command and watched-site guarantees, which require Milestones 7 through 9.

- [x] Define the internal `ExecutionEngine` interface, execution context,
  capabilities, requirements, result, diagnostics, assets, and provenance.
- [x] Implement the Jupyter discovery and startup foundation with
  `jupyter-zmq-client` and `jupyter-protocol`; discover and start only the
  explicitly configured kernel without requiring a Jupyter server.
  The internal adapter validates readiness and supervises bounded cleanup.
- [x] Execute the `CodeCell` nodes of one page sequentially in one page-scoped
  kernel session so definitions and imports persist between cells.
  The internal runner consumes prepared cells and waits for both terminal
  messages before advancing. Protocol fixtures and real Python and R tests
  verify retained state, separate page sessions, and cleanup. Public
  `ExecutionEngine` dispatch awaits output validation.
- [x] Validate the supported QMD metadata and option subset, including disabled
  and overridden declarations; prepare cells in source order with typed options,
  declaration ranges, and execution eligibility without kernel discovery or I/O.
- [x] Apply prepared evaluation, echo, output, include, and error behavior during
  execution and rendering; validate subcaption counts against final figures.
  The runner consumes prepared evaluation and error options; shared presentation
  views enforce visibility without deleting evidence. Final-output validation
  checks subcaptions before hiding output. The site renderer consumes these
  shared presentation views.
- [x] Collect stdout, stderr, execution errors, display data, display updates,
  and result MIME bundles into typed `CellOutput` nodes in protocol order.
  The internal incremental reducer retains stable slots, applies page-wide
  updates and clearing, orders validated MIME alternatives, and normalizes
  errors. Its validator boundary now retains safe Markdown/HTML and complete
  image bindings. The public engine now composes this validation with supervised
  cleanup, input revalidation, and portable provenance.
- [x] Treat ordinary streams as escaped preformatted text. Parse
  `text/markdown`, and explicitly as-is stream output, as isolated document
  fragments with execution disabled.
  The reducer parses Markdown MIME and adjacent as-is stdout runs, preserving
  fragment attribution and diagnostics. Literal output has an escaped
  preformatted rendering primitive. Reducer and protocol tests cover inert
  generated fences, stream boundaries, hidden output, and MIME fallbacks.
  The live adapter validates URLs and stages nested images before the next
  cell. Full site rendering remains its own step.
- [x] Store binary figures as content-addressed assets beneath an execution-
  output boundary; reject unsupported media, path traversal, and asset
  collisions deterministically.
  The page asset owner validates PNG, JPEG, and inert SVG, checks cached bytes,
  deduplicates safely, and retains only referenced assets. Reducer and protocol
  fixtures cover MIME fallback, boundary failures before the next cell,
  cancellation, and cleanup.
- [x] Sanitize supported `text/html` into a distinct IR representation before
  rendering, prefer a safe alternative MIME representation when available,
  and diagnose output that has no faithful safe representation.
  Reviewed HTML/Markdown validators and shared immutable result records bind
  safe content, typed diagnostics, canonical hashes, and verified image assets.
  The reducer retains those wrappers and nested assets through MIME fallback,
  display updates, and clearing. The public engine returns these validated
  records, and snapshot loading revalidates them before site rendering.
- [x] Add deterministic startup, idle, cell, and shutdown timeouts; interrupt
  failed execution, reap the kernel process, and preserve the last
  successful site during a watched-build failure.
  Real snapshot publication, site generation, and watched HTTP serving now
  connect the supervised engine to this failure boundary. See the
  [timeout and watched-site evidence](docs/design/execution-watched-validation.md)
  for deadline, cleanup, preservation, and recovery checks.
- [x] Implement a page-level execution cache keyed by authored source,
  normalized options, engine and kernel identity, relevant toolchain
  versions, and declared environment fingerprints. Validate cached assets
  before reuse.
  The public engine and executing commands now restore complete validated page
  artifacts after a current kernel handshake. Immutable atomic publication,
  corruption rejection, warning replay, and Python/R restoration are covered by
  the [cache validation evidence](docs/design/execution-cache-validation.md).
- [x] Record whether each page was executed or restored from cache without
  leaking connection files, ports, temporary paths, process IDs, timestamps,
  or absolute checkout paths into portable provenance.
- [x] Prove that `execution.mode = "never"` and every `diplodocus check` path
  avoid kernel discovery, startup, source execution, cache mutation, and
  execution-asset writes.
  Per-page Python/R origin checks, relocation checks, and filesystem event
  monitors establish these boundaries. See the
  [provenance and command-authority evidence](docs/design/execution-authority-validation.md).
- [ ] Add unit tests with a controllable protocol fixture and end-to-end tests
  with the declared Python and R kernels for success, state retention, rich
  output, timeout, interruption, missing kernels, unsupported MIME types,
  and deterministic cleanup.

**Exit gate:** Explicitly enabled Python and R QMD pages execute in source order
and produce reviewed structured-output snapshots; disabled and check-only paths
execute nothing; failure leaves no kernel or partial assets behind; and a cache
hit produces the same portable IR and assets as its originating execution.

## Milestone 7: Assemble, validate, and publish SQLite snapshots

The core owns workspace assembly and storage. Follow the [snapshot storage
design](docs/design/snapshots.md) and keep rendered routes, navigation, and page
layouts in the generation stage.

### Workspace assembly and checking

- [ ] Add failing tests for fragment conflicts, references, relationships,
  concepts, storage, and publication before implementing each behavior.
- [x] Assemble declared repositories, packages, extraction targets, authored
  collections and pages, diagnostics, and provenance into one workspace IR.
  Include authorized execution results when the engine is available.
  The assembly library now combines these sources and explicitly authorized
  execution. It owns staging across all pages and rejects changed inputs after
  cleanup. Static reference validation and `check` now consume this assembly.
  Snapshot publication and build/serve integration remain below.
- [x] Merge all extraction-target fragments for a package deterministically and
  diagnose duplicate or conflicting identities.
- [x] Resolve package-qualified references such as
  ``[`pyfoo::foo.FooModel.fit`]`` directly against semantic identities.
- [x] Resolve unqualified references first in the owning package and then in the
  workspace only when the result is unique; diagnose ambiguity and absence.
- [x] Resolve concept authoring names to item or callable-family IDs and retain
  the distinction between equivalent, analogous, and related concepts.
- [x] Validate package relationships and version constraints when both endpoints
  are present; report a known mismatch as an error and an indeterminate
  constraint as a warning.
- [x] Retain external relationship coordinates as provenance without treating a
  missing external source repository as an error.
- [x] Resolve authored page and local asset references against declared sources
  and retain portable targets for generation to map to URLs.
- [x] Wire `diplodocus check` through configuration, authored-content parsing,
  extraction, merging, reference resolution, and validation without
  executing a cell, publishing a snapshot, or creating the output directory.

### Snapshot storage and extraction

- [ ] Specify and document the SQLite tables, keys, relationships, serialized
  field shapes, and independent storage and IR schema versions. Make top-level
  entities queryable by semantic ID; use versioned serialized values for nested
  documents, signatures, and language extensions where appropriate.
- [ ] Implement snapshot writing and read-only loading with rejection of
  unsupported storage or IR versions. Validate required records, identities,
  paths, references, and asset fingerprints; defer automatic migrations.
- [ ] Store checked-in images and downloads and generated figure bytes by
  content fingerprint, with all references needed to recover them without
  source checkouts or execution caches.
- [ ] Include presentation defaults, slugs, content mounts, source-link
  information, diagnostics, and portable producer and repository provenance.
- [ ] Define versioned canonical record encodings and per-entity content
  fingerprints. Add a canonical text export for readable fixtures and logical
  snapshot comparisons, independent of SQLite file layout.
- [ ] Test semantic IR and asset round trips, deterministic fingerprints,
  unsupported schema versions, malformed records, missing assets, and corrupted
  asset contents.
- [ ] Publish a complete workspace atomically as a standalone database with no
  dependency on a live journal or write-ahead log. A failed refresh preserves
  the previous successful snapshot.
- [ ] Test repeated refreshes, stable IDs for retained entities, removal of
  stale records and unreferenced assets, and publication failure recovery.
  Refresh from source inputs without preserving manual database edits or
  accumulating historical snapshots.
- [ ] Wire `diplodocus extract` through assembly, configured execution,
  resolution, validation, asset collection, and snapshot publication. Implement
  the documented default path and `--output` override, and reject destinations
  that would overwrite declared inputs.
- [ ] Exclude generated snapshots and temporary storage files from source
  discovery and input fingerprints.
- [ ] Extend the acceptance registry and matrix with snapshot portability,
  refresh, validation, and failure scenarios, plus the separate `extract` and
  `generate` workflow.

### Contributor handoff checkpoint

- [ ] Provide a reproducible R/Python monorepo fixture with authored pages,
  semantic references, concepts, and a local asset. Configure its collections
  with `mode = "never"` so export needs no authored execution engine or runtime.
- [ ] Export that fixture through the real `extract` command and supply the
  database, its canonical text export, schema documentation, and example queries
  for packages, items, documents, references, and assets.
- [ ] Document the generator's input contract and the boundary between snapshot
  loading, site-model construction, and rendering, so frontend work can proceed
  independently of extraction.
- [ ] Copy the database to a directory without source checkouts and verify that
  the loader recovers the complete IR and asset bytes. Keep this artifact as a
  generation fixture for Milestone 8.

This checkpoint may precede production authored execution. Until that engine is
available, `extract` must report an error for collections requesting execution;
it must not silently publish a snapshot with their outputs missing. Completing
this checkpoint does not satisfy the full milestone's execution coverage.

**Exit gate:** `diplodocus check` succeeds for the valid acceptance workspace,
fails with the expected diagnostics for every invalid variant, publishes no
output, and produces the same ordered diagnostics on repeated runs. `extract`
publishes a complete, independently readable acceptance snapshot, including
authorized execution results and assets. Round trips, logical idempotence,
stale-record removal, schema validation, and failed-publication recovery pass.

## Milestone 8: Generate the coherent site from a snapshot

- [ ] Wire `diplodocus generate --input` through read-only snapshot loading and
  validation, site-model construction, and rendering. Use recorded presentation
  defaults and explicit overrides without consulting source configuration.
- [ ] Validate stored HTML against the active sanitizer policy before exposing
  it as renderable markup; deserialization alone grants no rendering trust.
- [ ] Add failing tests for routes, mount collisions, navigation, and visibility
  before implementing the site model.
- [ ] Assign stable routes independently of semantic IDs and diagnose route or
  mount collisions before rendering.
- [ ] Build project navigation from content collections and public packages,
  then build package navigation by ecosystem-aware item category.
- [ ] Keep internal items linkable and searchable without placing their package
  in the project switcher; keep hidden items linkable but absent from
  navigation and search.
- [ ] Construct renderer-ready page, breadcrumb, source-link, navigation,
  concept-switcher, code-cell-output, and search-entry models. Keep parsing,
  execution, and database access outside the renderer.
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
  syntax-highlighted display code and cell input, stream and error output,
  Markdown-valued output, sanitized HTML output, generated figures, and
  visible placeholders for unsupported content.
- [ ] Choose the renderer's safe MIME representation deterministically and prove
  that raw source HTML and unsanitized kernel HTML remain escaped or visibly
  unsupported.
- [ ] Generate a deterministic browser-side search index covering authored
  pages, packages, modules, types, functions, methods, signatures, and
  documentation text.
- [ ] Identify each search result by package and ecosystem and exclude hidden
  items.
- [ ] Bundle search behavior and all styling locally with usable keyboard,
  focus, contrast, narrow-screen, and no-JavaScript fallbacks.
- [ ] Generate site-local links independently of the hosting prefix so the same
  output works at an apex domain or beneath a GitHub Pages repository path.
- [ ] Restore content assets from the snapshot and combine them with the
  generator's bundled theme assets without reading source files.
- [ ] Render into a temporary sibling directory and replace the output only
  after successful generation. Reject an output path that would overwrite the
  input snapshot, and leave the snapshot unchanged on success or failure.
- [ ] Generate from copied databases without source checkouts, source
  configuration, execution caches, or language runtimes. Compare the resulting
  site files with generation beside the original sources.

**Exit gate:** `generate` produces the complete site from a copied SQLite
snapshot alone, leaving the input unchanged and preserving the previous site on
failure. Reviewed HTML snapshots cover both ecosystems and authored content;
all generated internal links resolve; search returns the expected cross-package
results; and no rendered page requires a network resource.

## Milestone 9: Complete `build` and `serve`

- [ ] Implement `diplodocus build` as extraction to the default snapshot path
  followed by generation. Reuse both stages and render only after successful
  snapshot publication.
- [ ] Prove that `build` and separate `extract` then `generate` produce the same
  logical snapshot, site files, and diagnostic outcomes for the same inputs.
- [ ] Preserve the previous snapshot on extraction failure and never generate
  from stale records after that failure. Preserve a successfully published
  snapshot and the previous site when generation fails.
- [ ] Reject an output path that overlaps the configuration file or any declared
  repository input.
- [ ] Write schema-versioned snapshot provenance into the output without
  timestamps or absolute local paths, including stable execution and cache
  provenance for QMD pages.
- [ ] Make CLI diagnostics concise by default and sufficiently detailed to find
  the responsible configuration or source location.
- [ ] Make `diplodocus serve` perform an initial build, bind only to its
  configured local address, and serve the successful output tree.
- [ ] Watch the configuration file and declared extraction, metadata, content,
  environment, and asset inputs; ignore generated snapshots, temporary storage
  files, output and execution-cache directories, and unrelated repository files.
- [ ] Debounce related filesystem events into one rebuild and keep the previous
  successful output available when parsing, execution, validation, or
  rendering fails.
- [ ] Handle address conflicts, deleted inputs, changed workspace configuration,
  missing tools or kernels, execution failure, and graceful process
  termination with stable diagnostics.
- [ ] Add integration tests for flag precedence, exit status, output
  replacement, HTTP serving, watched rebuilds, ignored changes, and recovery
  after a failed rebuild.

**Exit gate:** All five commands satisfy the interface contract in temporary
monorepo and multi-repository workspaces; `check` executes nothing and publishes
no output; `generate` uses only its snapshot and generator assets; `build`
matches the separate stages and runs only authorized cells; and `serve` observes
a source or declared-environment edit and exposes the new page without a restart
while preserving the last good site after an error.

## Milestone 10: Dogfood Diplodocus for its own documentation

Use Diplodocus---not another static-site generator---to build the documentation
users read about Diplodocus. This self-documentation workspace exercises
authored content and project navigation; documenting the Rust API remains
deferred until a Rust extractor exists.

- [x] Add a root `diplodocus.toml` that declares this checkout as a repository
  and mounts project-owned content from `docs/`.
- [ ] Support and test a content-only workspace with no API extraction targets
  so the self-documentation configuration does not pretend that Diplodocus
  has a Python or R public API.
- [ ] Make `docs/` the canonical source for the project overview, installation,
  quick start, workspace configuration, CLI, snapshot handoff and schema
  compatibility, GFM and QMD profiles, Python and R support, diagnostics,
  reproducibility, and the authored-execution security model.
- [ ] Link design and contributor material where useful instead of copying
  internal rationale into user documentation.
- [ ] Use the supported GFM and QMD features, one small deterministic executable
  page, navigation, checked-in and generated assets, source links, and
  search in the real site so regressions affect the project before they
  affect downstream users.
- [ ] Add tests that run the in-tree binary against the root `diplodocus.toml`,
  snapshot representative pages and the search index, and validate every
  local link and asset.
- [ ] Ensure the configured output directory and generated snapshot files are
  ignored and excluded from declared inputs so self-documentation builds cannot
  recurse into themselves.
- [ ] Use `diplodocus serve` as the documented local preview workflow for
  changes under `docs/`.
- [ ] Add `.github/workflows/docs.yml`, modeled on Basin's website workflow, to
  build and validate the Diplodocus site on pull requests, `main`, version
  tags, and manual dispatches.
- [ ] Upload and deploy only the Diplodocus-generated output through GitHub
  Pages; use the `github-pages` environment and the minimal `pages: write`
  and `id-token: write` permissions in the deployment job.
- [ ] Build the site for pull requests and `main`, but deploy only from `v*`
  version tags or an explicit manual dispatch, following Basin's separation
  of build verification from publication.
- [ ] Add `.nojekyll` as a deployment artifact without placing generated files
  in source control.
- [ ] Test assets, navigation, and search beneath the repository Pages path
  `/diplodocus/`; do not hard-code a deployment origin or assume an apex
  domain.

**Exit gate:** A clean checkout builds the complete project site with the
in-tree `diplodocus` binary, the result passes link and asset checks, local
preview uses `diplodocus serve`, and the GitHub Pages workflow deploys exactly
that generated tree without invoking another documentation generator.

## Milestone 11: Harden and release the MVP

- [ ] Run the complete acceptance workspace through `check`, `extract`,
  `generate`, `build`, and `serve` in end-to-end tests.
- [ ] Build the same declared inputs twice in different absolute directories and
  with fresh execution caches, then compare every site output path and byte and
  the snapshots' canonical records, fingerprints, and asset contents.
- [ ] Verify that `build` matches separate extraction and generation, including
  generation from a copied snapshot after removing its source checkout and
  execution cache from the disposable test workspace.
- [ ] Scan portable IR, snapshot records and assets, provenance, HTML, search
  data, and diagnostics for leaked absolute paths and nondeterministic metadata.
- [ ] Run normal command tests with network access disabled, fixtures whose API
  package imports or load hooks would fail if executed, and deterministic
  authored cells that require no network.
- [ ] Validate every internal HTML link, fragment, source-link shape, asset URL,
  navigation target, concept target, and indexed result.
- [ ] Exercise missing tools and kernels, malformed configuration and sources,
  unsupported constructs, cell timeouts and failures, unsafe and unsupported
  output, unsupported storage and IR versions, corrupted snapshots and assets,
  version mismatches, ambiguous references, publication failures, and output
  write failures.
- [ ] Review generated pages at narrow and wide viewport sizes and verify
  keyboard access, focus indication, heading order, labels, and color
  contrast.
- [ ] Document installation, workspace configuration, command behavior,
  diagnostics, the supported Python/R surface, the GFM/QMD profiles,
  supported cell options and MIME output, and the authored-execution
  security model in the dogfooded project site.
- [ ] Add a quick start that builds and previews the acceptance site from a
  clean checkout with declared tools already installed, and demonstrates
  exporting a snapshot for independent generation.
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
  - [ ] `diplodocus check` on the acceptance workspace;
  - [ ] `extract` and source-independent `generate` on the acceptance workspace;
  - [ ] equivalence of `build` and the separate stages;
  - [ ] two acceptance builds from fresh execution caches with byte-identical
    site output and logically identical snapshots; and
  - [ ] a byte-identical dogfood build plus its link and asset checker.

**Exit gate:** Every MVP completion criterion at the top of this file is
checked, the release gate passes from a clean checkout, and the documented quick
start reproduces both the acceptance site and Diplodocus's project site with no
network access required by Diplodocus or its deterministic authored cells.

## Architectural exclusions

- Runtime-based API introspection and external language-runtime parser helpers.
  Python, R, Julia, and other language runtimes are reserved for explicitly
  authorized execution of code examples and authored documentation chunks.

## Explicitly deferred until after the MVP

- Extraction caches and incremental extraction beyond `serve` rebuilding the
  current snapshot.
- Incremental rendering and its disposable dependency manifest.
- Automatic migration of older snapshot schemas.
- Browser live reload.
- Historical snapshot assembly and version switching.
- Rust, Julia, TypeScript, C, or other public-API extractors; until a Rust
  extractor exists, Diplodocus's dogfooded site documents its authored project
  and CLI material rather than generating Rust API reference pages.
- Automatic repository, package, or ecosystem discovery.
- A stable external extractor or renderer plugin API.
- A stable external execution-engine plugin API.
- Whole-document `.ipynb` input, mixed-kernel pages, knitr, interactive widgets,
  browser-side execution, and cell-level dependency analysis or caching.
- Checked-in frozen execution captures and trusted, unsanitized notebook HTML.
- Full Quarto, Pandoc, Sphinx, MyST, pkgdown, Documenter.jl, R Markdown, or
  arbitrary theme compatibility.
- Additional content adapters and trusted raw HTML.
- The `diplodocus init` command.
- A hosted documentation service operated by Diplodocus, repository management,
  package installation, dependency resolution, or implicit version-control
  operations.
