# Diplodocus: Design

## Purpose

Diplodocus is a documentation generator for polyglot software projects,
including both monorepos and product families spread across several
repositories.

Its central goal is to provide **one coherent documentation website** for
packages implemented in different languages, rather than composing sites
produced independently by tools such as rustdoc, pkgdown, Sphinx, or
Documenter.jl.

Typical projects include Python and R packages exposing the same statistical
library, a core library with bindings for several languages, and independently
released packages belonging to one software project.

Diplodocus should make these appear as parts of one documentation system, with
common navigation, styling, search, URLs, and page structure.

### Initial scope

The first release targets workspaces containing related Python and R packages.
It should prove the complete workflow for those two ecosystems before adding
others. Rust, Julia, and TypeScript are natural candidates for later extractors,
but are not part of the initial scope.

The initial product is one coherent snapshot of the documentation in the current
set of supplied source repositories. Extraction saves that snapshot in a
portable SQLite database; generation turns it into a static website. The two
stages can run independently, while `build` runs both. Historical release
assembly is a later concern.

## Core principles

### One renderer

Diplodocus owns the generated HTML. Extractors and content adapters supply a
shared structured model, giving the renderer control over navigation, search,
URLs, and language-specific presentation. Delegating HTML generation to rustdoc,
pkgdown, Sphinx, or Documenter.jl would divide that control among independent
systems and is outside the design.

Unsupported constructs must produce visible diagnostics rather than silent
information loss.

### Language-aware, not lowest-common-denominator

Diplodocus should have a common documentation model for concepts shared across
languages while retaining language-specific concepts where necessary.

For example:

- Rust: traits, impls, associated items, macros;
- Julia: generic functions and methods;
- R: S3/S4/R6 classes, generics, and methods;
- Python: classes, methods, decorators, protocols, overloads.

The unified UI should not require pretending these concepts are identical.

### Explicit workspaces

A workspace may contain any number and combination of packages from one or more
local source repositories. The repositories may be nested beneath the workspace
configuration or supplied as sibling checkouts:

```text
docs-workspace/
└── diplodocus.toml

checkouts/
├── foo-core/
├── foo-python/
└── foo-r/
```

Packages need not share a repository, language, or release cycle. Diplodocus
does not clone, fetch, or update repositories; the caller is responsible for
supplying the local source roots.

### Static output

The primary output is a self-contained static website suitable for GitHub Pages,
Cloudflare Pages, Netlify, or any ordinary HTTP server.

### Reproducible builds

The same declared source contents, configuration, extractor and parser versions,
capabilities, and environment inputs should produce the same extraction results,
apart from explicitly non-reproducible metadata.

The same logical snapshot, generator version, and presentation settings should
produce identical site files. Snapshot equivalence is defined by its records and
asset contents, not by the physical layout or bytes of the SQLite file.

Diplodocus itself should not perform implicit network access during normal
builds.

Authored execution adds its toolchain and results to this contract. Recording
provenance cannot make code deterministic when it reads undeclared state, uses
randomness or time, or accesses the network. A snapshot preserves the results
obtained, without claiming that another environment would reproduce them.

### Explicit authored execution

Language runtimes may be invoked only by execution engines for explicitly
authorized authored code. The MVP permits executable QMD cells; extracted API
examples remain display-only until they have a separate execution policy.
Parsing completes before execution and never depends on runtime results.

Authored code cells execute arbitrary code with the user's privileges. Execution
is therefore disabled by default and may be enabled only by workspace
configuration; document metadata alone cannot grant permission to execute. An
executing collection must select `mode = "execute"`, the `jupyter` engine, and
an explicit kernel name. The default mode is `never`. Diplodocus does not
sandbox cells or install their dependencies. Cells may access the network unless
the surrounding environment prevents it.

`diplodocus check` parses and validates code cells without executing them.
`extract`, including extraction invoked by `build` or `serve`, executes cells
only for content collections whose configuration explicitly enables execution.
`generate` never executes cells or starts a kernel.

--------------------------------------------------------------------------------

## Architecture

Diplodocus separates extraction and preparation from website generation. The
intermediate representation (IR) is the structured documentation model shared by
both stages; SQLite stores a complete snapshot of that model.

```text
Extraction
  checked-out source repositories and workspace configuration
      → static API extraction and authored-content parsing
      → optional authorized authored-cell execution
      → semantic reference resolution, validation, and asset collection
      → documentation.sqlite

Generation
  documentation.sqlite + presentation settings
      → snapshot validation
      → site model, URLs, navigation, and search
      → HTML renderer and static assets
```

The core coordinates extraction, merges results, resolves semantic references,
and publishes the snapshot only after successful validation.

Generation reads a completed snapshot without changing it. It requires neither
the source checkouts nor their configuration files, execution caches, or
language runtimes. A compatible Diplodocus binary supplies the renderer and
built-in theme. This boundary allows extraction and generation to run in
separate CI jobs or on different machines.

### API extractors

Each supported API ecosystem has an extractor that translates package metadata,
public APIs, and documentation into Diplodocus's intermediate representation. An
extractor operates on an explicit extraction target, not recursively on every
language found beneath a package root.

The distinction matters for mixed-language distributions. A Python package
implemented partly with a Rust extension is normally one Python documentation
package with a Python extraction target, not separate Python and Rust packages.
Internal bindings may be declared separately when their APIs are themselves
intended documentation targets.

Built-in extractors run in-process with Rust-native parsers and read only
declared inputs. They never start a language runtime or external parser, execute
package code, or invoke a build backend. Dynamic metadata and semantics outside
the supported static subset produce diagnostics, with no runtime fallback.

This keeps extraction usable without installing the documented packages or their
language environments, including native extensions available only as stubs. It
also leaves parser adaptation and semantic analysis under Diplodocus's control.
The cost is maintaining those adapters and accepting incomplete coverage of
dynamic APIs. The [static-extraction
decision](docs/decisions/0001-static-extraction.md) records the parser choices
and rejected alternatives.

For the initial extractors:

- Python uses static source analysis and package metadata. Public exports,
  re-exports, type stubs, and extension-module stubs form part of that static
  surface.
- R parses `DESCRIPTION`, `NAMESPACE`, maintained R source, and checked-in `Rd`
  documentation.

Extractors return package fragments independently of storage and rendering. The
[static-extractor contract](docs/spikes/static-extractor-contract.md) defines
their capabilities and provenance, including extractor and parser versions,
static mode, and diagnostics. These inputs also participate in any extraction
cache key.

### Authored content parsing

Diplodocus uses `panache-parser` in-process for authored Markdown. The adapter
selects its GFM or Quarto flavor and translates typed syntax views directly into
document IR, preserving source ranges and embedded-language diagnostics.
Unsupported syntax remains visible to the adapter.

Panache's CST stays inside this boundary. Neither its Pandoc projectors nor the
Panache, Pandoc, or Quarto command-line tools participate in the pipeline.
Direct translation preserves source evidence and cell declarations under the
adapter's control; the [authored-content
decision](docs/decisions/0002-authored-content.md) explains this choice.

### Documentation IR

All API extractors and authored-content adapters produce a common,
schema-versioned IR.

A simplified model is:

```text
Workspace
  Repository[]
  Package[]
  ContentCollection[]
  Page[]
  Concept[]
  PackageRelationship[]

Repository
  id
  canonical_url
  source_link_template
  revision
  declared_input_fingerprint

Package
  id
  slug
  name
  ecosystem
  version
  repository
  path
  metadata_path
  kind
  visibility
  extraction_targets[]
  items[]

ExtractionTarget
  id
  extractor
  path
  role

ContentCollection
  id
  owner
  repository
  path
  mount
  format
  execution: ExecutionConfiguration

ExecutionConfiguration
  mode
  engine
  kernel
  declared_environment_inputs[]

PackageRelationship
  from
  to
  kind
  version_constraint
  provenance

Item
  id
  kind
  name
  qualified_name
  signature: Signature
  documentation: Document
  source_location
  children[]
  language_data

Document
  metadata
  blocks[]
  source_format
  source_location

CodeCell
  language
  source
  options[]
  outputs[]
  source_location

CellOutput
  stream | display | error
  representations[]
  provenance

OutputRepresentation
  plain_text | markdown_blocks | asset | sanitized_html
  media_type
  content_or_asset
```

A repository represents one caller-supplied source root. Its local root is a
build input and is never written into portable IR or rendered output; source
locations use a repository ID and normalized repository-relative path. A package
represents a documented or released unit. Its `ecosystem` is the language of its
public API, not necessarily every implementation language present in its source
tree. A package `kind` is either `package` or `component`. Its `visibility` is
`public`, `internal`, or `hidden`, allowing an ABI used by a binding to
participate in semantic links without automatically appearing as a top-level
public package.

An extraction target identifies one authoritative API source within a package.
It may have a different root from the package metadata. Generated artifacts may
be targets when they are the only authoritative description of a public API, but
extractors should otherwise prefer maintained source and avoid indexing the same
API from both maintained and generated files. The target role distinguishes a
package's public API from an internal interface that is retained only for
cross-component documentation and links.

Signatures are structured syntax trees rather than display strings. Their nodes
represent parameters, defaults, return values, annotations, and
language-specific syntax. This allows the renderer and search index to use the
same semantic information without reparsing formatted text.

Documentation is also structured. A document contains blocks and inline nodes
for prose, display code, executable code cells, cell outputs, parameter and
return sections, admonitions, examples, and semantic references. Extractors and
content adapters should retain source-format provenance and raw source where it
is useful for diagnostics, but the renderer consumes the structured form.

Cell outputs distinguish escaped stream text, inert Markdown blocks,
content-addressed assets, and sanitized HTML. The [authored-execution
contract](docs/spikes/authored-execution-contract.md) defines conversion,
sanitization, and fallback behavior. Untyped output strings cannot bypass that
boundary.

Common item kinds include modules, functions, methods, types, classes,
constants, fields, and namespaces. Language-specific concepts belong in typed
extensions such as `PythonItemData` and `RItemData`.

### SQLite snapshot

The database is a portable documentation artifact, containing one complete
workspace snapshot. Source files and workspace configuration remain
authoritative when refreshing it; extraction does not preserve manual database
edits or append historical snapshots.

The snapshot contains:

- repository and package metadata, versions, and portable provenance;
- API items, signatures, authored documents, and recorded cell outputs;
- semantic references, concepts, package relationships, and diagnostics;
- the bytes of every local content asset needed by the site, including images,
  downloads, and generated figures;
- default presentation settings, package slugs, content mounts, and source-link
  information; and
- schema and producer versions, stable entity IDs, and content fingerprints.

SQLite keeps semantic records and asset bytes together in one transferable file
and supports atomic publication. It adds storage and validation code,
serialization costs, and a format compatibility obligation; snapshots with many
figures or downloads may be large. The initial scope accepts these costs for
portable artifacts and independent generation.

The core owns storage. Readers and writers reject unsupported IR or storage
schema versions; automatic migration is deferred. Generation validates records,
assets, paths, and references before constructing the site model, and
revalidates stored HTML against the active sanitizer policy. Built-in theme
assets ship with Diplodocus; external hyperlinks remain unfetched links.

### Snapshot updates

A refresh produces the same logical records and fingerprints from the same
declared inputs, implementation versions, and execution results. Stable IDs
identify retained entities; removed entities and unreferenced assets disappear.
The initial implementation may replace the whole snapshot.

Publication is atomic and produces a standalone file. A failed refresh leaves
the previous snapshot intact, but `build` and `serve` must report the failure
rather than silently generating from it. Storage idempotence does not eliminate
execution side effects or establish whether absent sources have changed.

The [snapshot storage design](docs/design/snapshots.md) specifies record
storage, canonical fingerprints and text exports, publication mechanics, and
provenance.

### Item identity

Every item ID is scoped by a stable package ID and assigned by its API
extractor. An item ID must distinguish overloaded functions, R methods and
generics, aliases, and other entities that may share a qualified name. It should
remain stable while the corresponding public API remains unchanged.

Item IDs are not URLs. The site model derives URLs from package slugs and item
metadata, which permits redirects and layout changes without changing semantic
references.

### Authored documentation

API reference documentation is only one part of the site. A workspace should
also support any number of authored content collections:

```text
core-repository/
└── docs/
    ├── index.md
    └── concepts.md

python-repository/
└── docs/
    ├── getting-started.md
    └── examples.md
```

Each collection has a project or package owner, a source repository, and a site
mount point. Ownership controls navigation and reference context; it is
independent of the repository in which the files happen to live. This permits,
for example, a project-level Python quickstart to live beside the core library.

The initial implementation supports two named input profiles:

- `gfm` reads `.md` files as a safe GitHub-Flavored Markdown subset. Fenced code
  is display-only.
- `qmd` reads `.qmd` files as a documented subset of Quarto Markdown. It adds
  Quarto executable fences with braced language names, hashpipe cell options,
  and the supported Quarto callout syntax.

These are compatibility profiles, not a new Diplodocus Markdown dialect.
Diplodocus does not promise every Quarto, Pandoc, R Markdown, MyST, or GFM
extension. Diplodocus semantic references are its only domain-specific inline
extension. Unsupported directives, metadata, cell options, and embedded
components produce visible diagnostics.

Each executable QMD page uses one configured Jupyter kernel, with cells run
sequentially in source order. Other-language blocks remain display-only. An
in-process Rust client consumes streams, errors, and display results without a
Jupyter server. This gives Python, R, and other installed kernels one execution
protocol; their executables and language packages remain external toolchain
requirements.

Execution attaches structured outputs to `CodeCell` nodes. Only Markdown-valued
results are parsed as isolated, non-executable fragments, preserving source
locations and preventing generated output from introducing executable cells. The
[authored-execution contract](docs/spikes/authored-execution-contract.md)
defines the supported metadata and options, output conversion, sanitization,
failure policy, and provenance.

The private execution cache stores complete pages of structured results to
preserve stateful cell semantics. The [page execution-cache
contract](docs/spikes/page-execution-cache.md) defines keys covering source,
options, engine, kernel, toolchain, and declared environment inputs, plus
validation and atomic publication. A hit verifies the current kernel through
startup and kernel info, then skips authored cells. Accepted outputs and asset
bytes enter the portable snapshot independently of cache entries.

Authored pages and generated API pages participate in the same navigation, link
resolution, and search index.

--------------------------------------------------------------------------------

## Workspace configuration

A workspace has one root configuration file. It may live in a dedicated
documentation repository or in any one of the source repositories:

```text
diplodocus.toml
```

For example:

```toml
[project]
name = "Foo"

[[repository]]
id = "core"
path = "../foo-core"
url = "https://github.com/example/foo-core"

[[repository]]
id = "python"
path = "../foo-python"
url = "https://github.com/example/foo-python"

[[repository]]
id = "r"
path = "../foo-r"
url = "https://github.com/example/foo-r"

[[package]]
id = "pyfoo"
name = "Foo for Python"
slug = "python"
ecosystem = "python"
repository = "python"
path = "."
metadata_path = "pyproject.toml"
targets = [
  { id = "api", extractor = "python", path = "python/foo", role = "public-api" },
]

[[package]]
id = "rfoo"
name = "Foo for R"
slug = "r"
ecosystem = "r"
repository = "r"
path = "."
metadata_path = "DESCRIPTION"
targets = [
  { id = "api", extractor = "r", path = ".", role = "public-api" },
]

[[content]]
id = "guide"
owner = "project"
repository = "core"
path = "docs"
mount = "guide"
format = "gfm"

[content.execution]
mode = "never"

[[content]]
id = "python-tutorials"
owner = "pyfoo"
repository = "python"
path = "docs/tutorials"
mount = "tutorials"
format = "qmd"

[content.execution]
mode = "execute"
engine = "jupyter"
kernel = "python3"
declared_environment_inputs = ["uv.lock"]
```

Repository paths may point outside the directory containing `diplodocus.toml`.
Package paths are relative to their repository roots; metadata and
extraction-target paths are relative to their package roots; content paths are
relative to their repository roots. All must remain within their declared
repository after normalization. This allows explicit sibling checkouts without
making an arbitrary relative path an undeclared source root.

The repository URL identifies the canonical source origin. An optional source
link template controls forge-specific revision, path, and line URLs; Diplodocus
may infer standard templates for known forges. It may read local version-control
metadata for provenance. Configuration or the build environment may supply the
revision when the source is not a version-control checkout.

The package `id` is the stable identity used by references and relationships.
The `slug` controls its URL and must be unique within the site. Neither is
derived from the package's ecosystem, so a workspace may contain several Python
or R packages, and packages in different ecosystems may share the same published
name. `kind` defaults to `package`, and `visibility` defaults to `public`. The
reserved content owner `project` denotes project-level material; any other owner
is a package ID.

The content `format` selects the [authored
documentation](#authored-documentation) profile. Execution follows the
[workspace authorization policy](#explicit-authored-execution); frontmatter may
configure supported presentation and cell behavior within that authority.

Declared environment inputs are paths relative to the content collection's
repository and obey the same traversal and symlink restrictions as other
declared inputs. They commonly include lockfiles or environment manifests. Their
contents participate in provenance and execution-cache keys, but Diplodocus does
not interpret them or install the environment they describe.

Configuration remains authoritative; automatic package discovery is deferred.

--------------------------------------------------------------------------------

## Build snapshots and versions

A workspace contains multiple packages, and their versions need not match. A
normal build represents one snapshot of the current set of source repositories
and records the version of each package independently.

For example:

```text
pyfoo  2.1.0
rfoo   1.8.0
```

A site generated from that snapshot might look like:

```text
/
├── guide/
└── packages/
    ├── python/
    └── r/
```

A multi-repository snapshot is coherent only when its binding and dependency
constraints match the versions represented by the supplied sources. Package
relationships let `diplodocus check` diagnose known mismatches.

Historical documentation requires assembling snapshots built from different sets
of source revisions. That operation is outside the initial `build` command. A
later release assembler may consume existing snapshot artifacts or explicit
checkouts, but it must not make ordinary builds depend on implicit
version-control operations or network access.

--------------------------------------------------------------------------------

## Navigation

Navigation should have two distinct levels.

The project level:

```text
Guide
Foo for Python
Foo for R
```

The package level:

```text
Foo for Python
├── Overview
├── Functions
├── Classes
└── Modules
```

Navigation is generated from packages in the site model rather than from its set
of ecosystems or independently by each extractor.

Only public packages appear at the project level by default. Internal units may
contribute reference pages, semantic link targets, and search entries without
appearing in the package switcher. Hidden units contribute semantic targets but
no independently discoverable navigation or search results. Configuration may
raise the visibility of a component when its interface is intended for users.

The interface should identify each package's API ecosystem and provide a package
switcher where appropriate. Pages belonging to a conceptual API group should
also provide a direct switcher between that concept's implementations.

--------------------------------------------------------------------------------

## Package and component relationships

Packages and internal components may have typed relationships such as:

```text
depends-on
binds
wraps
generated-from
```

A relationship may include a declared version constraint and provenance for
where it was discovered, such as package metadata or explicit configuration.
These relationships describe implementation and release structure; they are
distinct from conceptual relationships between individual API items.

A relationship endpoint may be another workspace package ID or an external
ecosystem-qualified package coordinate. External relationships remain useful
provenance even when no corresponding source repository is present; version
compatibility can be checked only when the other endpoint and its version are
available.

For example, a Python distribution may bind a Rust crate compatible with version
`1.9`, while a Julia package consumes an artifact from one exact C ABI release.
Recording both relationships lets validation distinguish the source version
currently being documented from the dependency version actually used by each
binding.

Diplodocus does not attempt dependency resolution. It reports inconsistent or
unknown relationships when enough information is available and otherwise retains
them as snapshot metadata.

--------------------------------------------------------------------------------

## Cross-package relationships

Diplodocus should support relationships between equivalent or related APIs
across packages in its first useful release.

For example:

```text
pyfoo  FooModel.fit
rfoo   fit.foo_model
```

These relationships are initially declared explicitly as conceptual API groups:

```toml
[[concept]]
id = "foo-model.fit"
kind = "analogous"
members = [
  { package = "pyfoo", item = "foo.FooModel.fit" },
  { package = "rfoo", item = "fit.foo_model" },
]
```

A concept may be `equivalent`, `analogous`, or `related`. The kind controls how
the UI describes the relationship; it must not claim that two APIs are the same
when they merely provide comparable capabilities.

A concept may contain any number of members from any combination of packages,
including several packages written in the same language. Members resolve to
semantic item IDs during validation. Qualified names are accepted as authoring
conveniences only when they resolve unambiguously.

Concept members normally target a public callable family. A family may contain a
generic and its methods, or a function and its overloads, so that languages with
different dispatch models do not require one concept per method. The API
extractor defines these family relationships in the package IR while preserving
every method or overload as an addressable item.

Concept pages and member pages should expose the relationship prominently, for
example through a "Same API in" switcher for equivalent members or a "Related
API" switcher for analogous members. Automatic inference may be considered
later, but is not required for the initial design.

This feature is especially valuable for binding-oriented repositories and is a
principal differentiator from existing documentation generators.

--------------------------------------------------------------------------------

## Linking

Every documentation entity has a stable internal identifier independent of its
rendered URL. Links resolve against those identifiers, supporting cross-package
references, renames and redirects, source links, and validation.

Authored content should support package-qualified semantic references:

```text
[`pyfoo::foo.FooModel.fit`]
```

An unqualified shorthand such as ``[`FooModel.fit`]`` may be accepted when it
resolves unambiguously in the current package or workspace. Ambiguous references
are errors reported by `diplodocus check`.

--------------------------------------------------------------------------------

## Search

Search covers authored pages, packages, modules, types, functions, methods,
signatures, and documentation text across the workspace. Results identify both
package and API ecosystem. The first implementation can generate a static
browser-side index.

--------------------------------------------------------------------------------

## Rendering

Generation loads and validates the snapshot, then constructs the site model. The
renderer consumes only that model and must not contain language parsing or
database access logic.

Default presentation settings travel with the snapshot. Explicit generation
options may override presentation without repeating extraction; changes to
source selection, API semantics, or execution policy require a new extraction.
The initial renderer has one built-in theme. This boundary permits later theme
support without making a theme extension API part of the first release.

The renderer owns HTML, responsive layout, navigation, breadcrumbs, search,
source links, syntax highlighting, API signatures, and code-cell presentation.
Language-specific components allow a Python class page and an R generic-function
page to use different layouts within one visual system.

Display results use the safe representations defined by the document IR and
execution contract. Raw source HTML and unsanitized kernel HTML never enter the
renderer as trusted markup.

### Incremental rendering

The initial generator may render the entire site. Snapshots must preserve stable
entity IDs, per-entity content fingerprints, and structured references for later
dependency tracking. Selective generation must produce the same files as a clean
full generation. The [future rendering
design](docs/design/snapshots.md#future-incremental-rendering) describes the
proposed manifest and invalidation behavior.

--------------------------------------------------------------------------------

## CLI

The CLI exposes both stages and a convenient combined workflow:

```text
diplodocus build
diplodocus serve
diplodocus check
diplodocus extract --output documentation.sqlite
diplodocus generate --input documentation.sqlite --output site
```

`extract` runs the [extraction pipeline](#architecture). Its default output is
`.diplodocus/documentation.sqlite` relative to the workspace configuration;
`--output` selects another snapshot path.

`generate` requires an explicit `--input` snapshot and renders it to `--output`,
which defaults to `./site`. It uses recorded defaults and explicit presentation
overrides. Both stages report diagnostics and return a nonzero exit status on
errors.

`build` runs extraction into the default snapshot location followed by
generation. It must have the same behavior as running the two stages separately.
`serve` builds, serves, and watches declared inputs, retaining the last
successful site when a rebuild fails. A failed generation also leaves the last
successful site intact; a successfully extracted snapshot remains usable for
another generation attempt.

`check` should validate configuration, source roots, unresolved references,
duplicate identifiers, missing package metadata, incompatible package
relationships, unsupported content constructs and cell options, execution
configuration, and similar documentation problems without executing cells,
publishing a snapshot, or producing a site. It shares parsing, extraction, and
validation components with `extract` but cannot validate references introduced
only by execution results that do not yet exist.

`diplodocus init` may be added later.

--------------------------------------------------------------------------------

## Extensibility

The core defines an internal extractor interface conceptually similar to:

```text
Extractor
  ecosystem()
  capabilities() -> ExtractionCapabilities
  extract(context, package, target) -> ExtractionResult

ExtractionResult
  package_fragment
  diagnostics[]
  provenance
```

The core merges fragments from a package's extraction targets and diagnoses
duplicate or conflicting item identities. Extractors do not merge fragments or
infer undeclared targets themselves.

Authored execution is modular through a separate internal interface:

```text
ExecutionEngine
  name()
  requirements() -> ToolchainRequirement[]
  capabilities() -> ExecutionCapabilities
  execute(context, document) -> ExecutionResult

ExecutionResult
  document_with_outputs
  supporting_assets[]
  diagnostics[]
  provenance
```

Engines return document IR and assets without modifying source files or emitting
page HTML. Both interfaces can initially live in the main repository; stable
external plugin APIs are deferred until the internal contracts have matured.

--------------------------------------------------------------------------------

## Implementation strategy

Implement a vertical slice through related Python and R packages in separate
repositories. Diplodocus's CLI, core, renderer, extractors, content adapter, and
Jupyter client will use Rust, providing one binary for the complete pipeline.
The [roadmap](TODO.md) tracks milestones and fixtures; the architectural
sequence is:

1. Build an acceptance corpus covering Python functions, classes, re-exports,
   maintained and native-extension stubs, R functions and S3 methods, GFM,
   executable QMD, unsupported constructs, and equivalent and analogous APIs.
2. Spike extraction, authored parsing, and execution to expose information loss,
   then define the structured IR and identity rules from that evidence.
3. Implement both extractors test-first against golden IR fixtures, followed by
   content adapters, execution, semantic resolution, and `check` diagnostics.
4. Implement snapshot storage and `extract`. Test IR and asset round trips,
   schema validation, idempotence, stale-record removal, and failure recovery.
   Include stable IDs, fingerprints, and canonical text exports from the outset.
5. Implement `generate`, shared navigation, search, and concept switchers.
   Verify generation from a copied database without sources, caches, or
   runtimes.
6. Verify deterministic end-to-end output, including deterministic executable
   cells, and equivalence of `build` with separate `extract` and `generate`.

Incremental processing, custom themes, historical release assembly, and further
ecosystems follow this complete workflow. A C extractor is optional: authored
reference content and an internal component can initially represent a C ABI.

--------------------------------------------------------------------------------

## Non-goals

At least initially, Diplodocus is not:

- a general-purpose static site generator;
- a replacement for Markdown authoring;
- a package manager;
- a repository manager or dependency resolver;
- a build system for the packages themselves;
- a historical documentation release assembler;
- a hosted documentation service;
- an IDE documentation engine;
- a universal source-code parser;
- a compatibility layer for existing Sphinx/pkgdown/Documenter themes or
  extensions;
- a complete Quarto, Pandoc, R Markdown, or Jupyter implementation;
- a package or kernel installer;
- a sandbox for authored code;
- a host for interactive widgets or arbitrary trusted notebook HTML.

The focus is deliberately narrow:

> Generate one coherent documentation snapshot from supplied repositories for
> related Python and R packages, with explicit links between corresponding APIs.
