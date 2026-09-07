# Diplodocus: Design

## Purpose

Diplodocus is a documentation generator for polyglot software projects, including
both monorepos and product families spread across several repositories.

Its central goal is to provide **one coherent documentation website** for
packages implemented in different languages, rather than composing sites
produced independently by tools such as rustdoc, pkgdown, Sphinx, or
Documenter.jl.

Typical projects include:

- Python and R packages that expose the same statistical library;
- a core library with bindings for several programming languages;
- several independently released packages in one monorepo;
- several independently released packages in separate repositories;
- packages written in different languages but belonging to the same software
  project.

Diplodocus should make these appear as parts of one documentation system, with
common navigation, styling, search, URLs, and page structure.

### Initial scope

The first release targets workspaces containing related Python and R packages.
It should prove the complete workflow for those two ecosystems before adding
others. Rust, Julia, and TypeScript are natural candidates for later extractors,
but are not part of the initial scope.

The initial product is one coherent snapshot of the documentation in the
current set of supplied source repositories. Historical release assembly is a
later concern.

## Core principles

### One renderer

Diplodocus owns the generated HTML.

Package metadata, source code, stubs, namespaces, and documentation formats are
parsed in-process with Rust libraries or Diplodocus-owned Rust parsers. Language
runtimes are reserved for explicitly authorized execution of code examples and
authored documentation chunks; they are not part of parsing or API extraction.
Diplodocus must not delegate HTML generation to rustdoc, pkgdown, Sphinx,
Documenter.jl, or equivalent systems.

When an extractor encounters a construct that Diplodocus cannot faithfully
represent, it should emit a visible diagnostic rather than silently discarding
information.

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

Packages need not share a repository, language, or release cycle. Diplodocus does
not clone, fetch, or update repositories; the caller is responsible for
supplying the local source roots.

### Static output

The primary output is a self-contained static website suitable for GitHub Pages,
Cloudflare Pages, Netlify, or any ordinary HTTP server.

### Reproducible builds

The same source repositories and configuration should produce the same
documentation output, apart from explicitly non-reproducible metadata.

Diplodocus itself should not perform implicit network access during normal builds.

Executable authored content weakens this guarantee in a visible, controlled
way. Diplodocus records the selected execution engine, kernel, toolchain,
normalized cell options, declared environment inputs, and source fingerprint in
provenance and execution-cache keys. It cannot make code deterministic when the
code reads undeclared state, uses randomness or time, or accesses the network.

### Explicit authored execution

Language runtimes may be invoked only by execution engines, and only to run
explicitly authorized examples or authored documentation code chunks. The MVP
authorizes executable QMD cells; extracted API examples remain display-only
until they have a separate execution policy. Parsing completes before execution
and never depends on runtime results.

Authored code cells execute arbitrary code with the user's privileges. Execution
is therefore disabled by default and may be enabled only by workspace
configuration; document metadata alone cannot grant permission to execute.
Diplodocus does not sandbox cells, install their dependencies, or make network
requests on their behalf. Because execution is unsandboxed, however, a cell may
access the network unless the surrounding environment prevents it.

`diplodocus check` parses and validates code cells without executing them. `build`
and `serve` execute cells only for content collections whose configuration
explicitly enables execution.

--------------------------------------------------------------------------------

## Architecture

The main data flow is:

```text
checked-out source repositories
      │
      ▼
packages, extraction targets, and authored content
      │
      ├── API extractors
      └── Panache content adapters
      │
      ▼
documentation IR
      │
      ├── optional authored-cell execution
      ├── cross-package concepts
      └── package version metadata
      │
      ▼
site model
      │
      ▼
HTML renderer
```

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

Initial extractors:

```text
Python
R
```

Possible later extractors include Rust, Julia, and TypeScript.

Extractors use Rust-native parsing infrastructure. They may preserve and model
ecosystem-specific semantics, but they do not invoke the documented language's
runtime to discover those semantics.

For the initial extractors:

- Python uses static source analysis and package metadata. Public exports,
  re-exports, type stubs, and extension-module stubs form part of that static
  surface. Extraction never imports the documented package.
- R parses `DESCRIPTION`, `NAMESPACE`, maintained R source, and checked-in `Rd`
  documentation without starting R or loading the documented package.

The same boundary applies to later ecosystems. When Rust-native extraction
cannot represent a required dynamic construct, the extractor emits a visible
diagnostic rather than falling back to runtime introspection or an external
parser helper.

The extractor boundary should remain independent from the renderer.

Conceptually:

```text
diplodocus extract python ./python/package
         │
         ▼
   package fragment
```

### Static extractor boundary

Built-in extractors run in the Diplodocus process. They may read only declared
inputs and do not start a language runtime, execute package code, invoke a build
backend, import a Python package, or source, attach, or load an R package.
Dynamic metadata and semantics outside a supported static subset produce
diagnostics.

The generated IR records the extractor and parser versions, declared
capabilities, static extraction mode, and diagnostics. These inputs also form
part of any extraction cache key.

Reproducibility means that the same source repository contents, configuration,
extractor and parser versions, capabilities, and declared environment inputs
produce the same output. External runtimes and toolchains affect this contract
only when explicitly authorized code examples or authored code chunks are
executed.

### Authored content parsing

Diplodocus uses the `panache-parser` Rust crate in-process for authored Markdown. It
does not invoke Panache's command-line interface, Pandoc, or Quarto. The content
adapter selects Panache's GFM or Quarto flavor, consumes its typed syntax views
and embedded-language diagnostics, and translates supported constructs directly
into Diplodocus's document IR.

The Panache CST is a source-facing representation, not Diplodocus's portable IR.
Diplodocus does not use Panache's Pandoc-native or Pandoc-JSON projectors as an
interchange format. Unsupported and newly introduced syntax must remain visible
to the adapter with its source range so that Diplodocus can diagnose it rather than
silently flattening or discarding it.

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
locations use a repository ID and normalized repository-relative path. A
package represents a documented or released unit. Its `ecosystem` is the
language of its public API, not necessarily every implementation language
present in its source tree. A package `kind` is either `package` or `component`.
Its `visibility` is `public`, `internal`, or `hidden`, allowing an ABI used by a
binding to participate in semantic links without automatically appearing as a
top-level public package.

An extraction target identifies one authoritative API source within a package.
It may have a different root from the package metadata. Generated artifacts may
be targets when they are the only authoritative description of a public API,
but extractors should otherwise prefer maintained source and avoid indexing the
same API from both maintained and generated files. The target role distinguishes
a package's public API from an internal interface that is retained only for
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

Cell output is never an untyped HTML or Markdown string passed to the renderer.
Ordinary stdout and stderr become escaped, preformatted stream output. A
`text/markdown` representation, or stdout explicitly marked as `output: asis`,
is parsed as a Markdown fragment with execution disabled and stored as document
blocks. Binary figures become content-addressed local assets. HTML output must
be sanitized into a distinct representation before reaching the renderer; when
safe sanitization would lose the result's meaning, Diplodocus emits a diagnostic
and falls back to another supported MIME representation.

Common item kinds might include:

```text
module
function
method
type
class
constant
field
namespace
```

Language-specific concepts belong in typed language extensions rather than being
flattened into generic concepts.

For example:

```text
PythonItemData
RItemData
```

The IR should have an explicit schema version so extractors and renderers can
evolve independently.

### Item identity

Every item ID is scoped by a stable package ID and assigned by its API
extractor. An item ID must distinguish overloaded functions, R methods and
generics, aliases, and other entities that may share a qualified name. It should
remain stable while the corresponding public API remains unchanged.

Every source location is scoped by a repository ID and uses a normalized path
relative to that repository root. Portable IR and generated output must not
contain machine-specific absolute checkout paths.

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

- `gfm` reads `.md` files as a safe GitHub-Flavored Markdown subset. Fenced
  code is display-only.
- `qmd` reads `.qmd` files as a documented subset of Quarto Markdown. It adds
  Quarto executable fences with braced language names, hashpipe cell options,
  and the supported Quarto callout syntax.

These are compatibility profiles, not a new Diplodocus Markdown dialect. Diplodocus
does not promise every Quarto, Pandoc, R Markdown, MyST, or GFM extension.
Diplodocus semantic references are its only domain-specific inline extension.
Unsupported directives, metadata, cell options, and embedded components produce
visible diagnostics.

Only `qmd` collections may contain executable cells. Each executable page uses
one configured Jupyter kernel, and its cells run sequentially in source order in
one page-scoped session. Code blocks for other languages remain display-only;
multiple executable kernels within one page are outside the initial scope.
Kernel-backed execution provides a language-neutral protocol for Python, R, and
other installed kernels without making Quarto, Pandoc, or a Jupyter server a
Diplodocus dependency.

The first execution implementation consumes Jupyter streams, errors, display
data, and result MIME bundles through an in-process Rust client. Kernel
executables and language packages remain declared external toolchain
requirements. Diplodocus never installs a kernel or its dependencies.

Execution transforms `CodeCell` nodes in the document IR by attaching structured
outputs. It does not generate an intermediate Markdown file or reparse the
complete authored page. Markdown-valued results are parsed only as isolated,
non-executable fragments. This preserves original source locations and prevents
generated output from introducing another executable cell.

An execution cache stores a complete page's structured cell results rather than
generated Markdown. Its key includes the authored source, normalized options,
engine and kernel identities, relevant toolchain versions, and declared
environment inputs. Page-level caching preserves stateful cell semantics; fine-
grained dependency analysis and cell-level caching are later concerns.

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
link template controls forge-specific revision, path, and line URLs; Diplodocus may
infer standard templates for known forges. Diplodocus records a revision and a
fingerprint of the declared extraction and content inputs for every repository
in the generated provenance. It may inspect local version-control metadata
without modifying the checkout, but it never fetches or changes revisions.
Configuration or the build environment may provide the revision when the source
is not a version-control checkout.

The package `id` is the stable identity used by references and relationships.
The `slug` controls its URL and must be unique within the site. Neither is
derived from the package's ecosystem, so a workspace may contain several
Python or R packages, and packages in different ecosystems may share the same
published name. `kind` defaults to `package`, and `visibility` defaults to
`public`. The reserved content owner `project` denotes project-level material;
any other owner is a package ID.

The content `format` is explicit: `gfm` collections discover `.md` files, and
`qmd` collections discover `.qmd` files. Execution defaults to `mode = "never"`.
The initial execution modes are `never` and `execute`; `execute` is valid only
for `qmd` and requires the `jupyter` engine and an explicit kernel name.
Document frontmatter may configure supported presentation and cell behavior but
cannot select an execution mode, engine, or kernel that the collection did not
authorize.

Declared environment inputs are paths relative to the content collection's
repository and obey the same traversal and symlink restrictions as other
declared inputs. They commonly include lockfiles or environment manifests.
Their contents participate in provenance and execution-cache keys, but Diplodocus
does not interpret them or install the environment they describe.

Configuration should be explicit and small.

Automatic package discovery may be added later, but the configuration file
remains authoritative.

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

A generated snapshot might look like:

```text
/
├── guide/
└── packages/
    ├── python/
    └── r/
```

The snapshot provenance records each repository's canonical URL, revision,
declared-input fingerprint, and dirty state when available, together with each
package's extracted version and declared relationships. For executed content it
also records the engine, kernel, kernel-reported language and version, cell
options, declared environment fingerprints, and whether an output came from a
fresh execution or the page-level cache. A multi-repository snapshot is
coherent only when its binding and dependency constraints match the versions
represented by the supplied sources. `diplodocus check` should diagnose known
mismatches but must not resolve, install, or update dependencies.

Historical documentation requires assembling snapshots built from different
sets of source revisions. That operation is outside the initial `build`
command. A later release assembler may consume existing snapshot artifacts or
explicit checkouts, but it must not make ordinary builds depend on implicit
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

Navigation is generated from packages in the site model rather than from its
set of ecosystems or independently by each extractor.

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

For example, a Python distribution may bind a Rust crate compatible with
version `1.9`, while a Julia package consumes an artifact from one exact C ABI
release. Recording both relationships lets validation distinguish the source
version currently being documented from the dependency version actually used
by each binding.

Diplodocus does not attempt dependency resolution. It reports inconsistent or
unknown relationships when enough information is available and otherwise
retains them as snapshot metadata.

--------------------------------------------------------------------------------

## Cross-package relationships

Diplodocus should support relationships between equivalent or related APIs across
packages in its first useful release.

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

Concept members normally target a public callable family. A family may contain
a generic and its methods, or a function and its overloads, so that languages
with different dispatch models do not require one concept per method. The
API extractor defines these family relationships in the package IR while
preserving every method or overload as an addressable item.

Concept pages and member pages should expose the relationship prominently, for
example through a "Same API in" switcher for equivalent members or a "Related
API" switcher for analogous members. Automatic inference may be considered
later, but is not required for the initial design.

This feature is especially valuable for binding-oriented repositories and is a
principal differentiator from existing documentation generators.

--------------------------------------------------------------------------------

## Linking

Every documentation entity must have a stable internal identifier independent of
its rendered URL.

Links should therefore resolve against semantic identifiers rather than raw HTML
paths.

This enables:

- cross-package references;
- API renames and redirects;
- automatic source links;
- link validation.

Authored content should support package-qualified semantic references:

```text
[`pyfoo::foo.FooModel.fit`]
```

An unqualified shorthand such as ``[`FooModel.fit`]`` may be accepted when it
resolves unambiguously in the current package or workspace. Ambiguous
references are errors reported by `diplodocus check`.

--------------------------------------------------------------------------------

## Search

Search should operate over the entire documentation workspace.

The search index should include:

```text
authored pages
packages
modules
types
functions
methods
signatures
documentation text
```

Search results should identify both package and API ecosystem.

The first implementation can generate a static browser-side search index.

--------------------------------------------------------------------------------

## Rendering

The renderer consumes only the site model and must not contain language parsing
logic.

It is responsible for:

- HTML;
- layout;
- navigation;
- syntax highlighting;
- code-cell inputs and structured outputs;
- API signatures;
- source links;
- breadcrumbs;
- search;
- responsive design.

Language-specific presentation should be implemented through structured renderer
components rather than separate themes.

The renderer chooses among the safe representations retained for a display
result. It escapes text, renders parsed Markdown blocks through the ordinary
document path, emits local content-addressed assets, and accepts HTML only from
the sanitizer boundary. Raw source HTML and unsanitized kernel HTML never enter
the renderer as trusted markup.

For example, a Python class page and an R generic-function page may use different
layouts while clearly belonging to the same visual system.

--------------------------------------------------------------------------------

## CLI

The initial CLI should remain small:

```text
diplodocus build
diplodocus serve
diplodocus check
```

Potential additional commands:

```text
diplodocus init
diplodocus extract
```

`build` should perform extraction, authored-content parsing, configured cell
execution, validation, site construction, and rendering.

`check` should validate configuration, source roots, unresolved references,
duplicate identifiers, missing package metadata, incompatible package
relationships, unsupported content constructs and cell options, execution
configuration, and similar documentation problems without executing cells or
producing a site.

--------------------------------------------------------------------------------

## Extensibility

API ecosystem support should be modular.

The core should define an extractor interface conceptually similar to:

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

Configuration is authoritative, so automatic package or ecosystem detection is
not part of the initial interface. Extractors should not have access to
rendering internals.

Initially, extractors can live in the main repository. A stable external plugin
API is unnecessary until the internal IR and extractor API have matured.

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

The initial engine is `jupyter`. It consumes the document IR, executes its cells
in one page-scoped kernel session, and returns structured outputs. Engines do not
emit page HTML, mutate the source document, install dependencies, or bypass the
renderer. This internal interface does not imply a stable external execution-
engine plugin API.

--------------------------------------------------------------------------------

## Implementation strategy

The initial implementation should be a vertical slice through one representative
workspace containing related Python and R packages in separate source
repositories. A sensible order is:

1. Define a small multi-repository acceptance corpus containing Python functions
   and classes, public re-exports and type stubs, a native-extension stub, R
   functions and S3 methods, authored GFM and executable QMD, unsupported
   content directives, and several equivalent and analogous APIs.
2. Spike both API extractors, the Panache content adapter, and Jupyter execution
   against that corpus to discover what their Rust-native libraries expose and
   where information is lost.
3. Define the repository, package, extraction-target, content-collection, and
   relationship models, together with the structured IR, stable item IDs, and
   conceptual API groups, code cells, output representations, and execution
   provenance from the observed data.
4. Implement the Python and R extractors test-first against golden IR fixtures.
5. Implement the GFM and QMD adapters, followed by page-scoped Jupyter execution
   and its structured output conversion.
6. Implement semantic reference resolution and `diplodocus check`, including
   diagnostics for ambiguity, unsupported constructs, incoherent package
   relationships, and unresolved concepts.
7. Render authored pages, code-cell outputs, and both API references in one
   site.
8. Add package navigation, static workspace search, and the appropriate concept
   switchers.
9. Add end-to-end snapshot tests that verify deterministic output from the
   acceptance workspace, including deterministic executable cells.
10. Only then consider historical release assembly or another ecosystem.

Diplodocus's CLI, core, renderer, built-in extractors, Panache adapter, and Jupyter
client will be implemented in Rust. This provides a convenient single binary
and fits well with parsing, static-site generation, and concurrent builds. All
built-in extractors parse their inputs in-process with Rust-native
infrastructure. An execution engine alone may start an explicitly configured
external Jupyter kernel, subject to the code-execution contract. Kernel
executables and language packages are execution toolchain requirements; they
are not extractor dependencies and do not replace Diplodocus's Rust implementation
or renderer.

Rust, Julia, and TypeScript are the next natural public-API extractors for a
core-with-bindings ecosystem. A C extractor is optional: a C ABI may instead be
represented initially by authored reference content and an internal component.

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
