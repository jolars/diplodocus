# Polydoc: Design

## Purpose

Polydoc is a documentation generator for polyglot software projects, including
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

Polydoc should make these appear as parts of one documentation system, with
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

Polydoc owns the generated HTML.

Language-specific tooling may be used to obtain semantic information, but
Polydoc must not delegate HTML generation to rustdoc, pkgdown, Sphinx,
Documenter.jl, or equivalent systems.

When an extractor encounters a construct that Polydoc cannot faithfully
represent, it should emit a visible diagnostic rather than silently discarding
information.

### Language-aware, not lowest-common-denominator

Polydoc should have a common documentation model for concepts shared across
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
└── polydoc.toml

checkouts/
├── foo-core/
├── foo-python/
└── foo-r/
```

Packages need not share a repository, language, or release cycle. Polydoc does
not clone, fetch, or update repositories; the caller is responsible for
supplying the local source roots.

### Static output

The primary output is a self-contained static website suitable for GitHub Pages,
Cloudflare Pages, Netlify, or any ordinary HTTP server.

### Reproducible builds

The same source repositories and configuration should produce the same
documentation output, apart from explicitly non-reproducible metadata.

Polydoc should not perform implicit network access during normal builds.

--------------------------------------------------------------------------------

## Architecture

The system consists of four major layers:

```text
checked-out source repositories
      │
      ▼
packages and extraction targets
      │
      ▼
API extractors
      │
      ▼
documentation IR
      │
      ├── guides / authored content
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
public APIs, and documentation into Polydoc's intermediate representation. An
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

Extractors should use native semantic infrastructure where useful.

For the initial extractors:

- Python should use static source analysis and package metadata by default.
  Public exports, re-exports, type stubs, and extension-module stubs form part of
  that static surface. Import-based introspection is an explicit optional mode.
- R should read `DESCRIPTION` and `NAMESPACE` metadata and consume parsed `Rd`
  documentation without loading the package by default. Runtime introspection
  is an explicit optional mode.

The extractor boundary should remain independent from the renderer.

Conceptually:

```text
polydoc extract python ./python/package
         │
         ▼
   package fragment
```

### Extractor execution

Extractors must declare their toolchain requirements and whether they operate
statically or execute package code. Static extraction is preferred. Runtime
introspection may be enabled when an ecosystem cannot otherwise expose the
required semantics, but it must be explicit because importing a Python package
or loading an R package can execute arbitrary code.

A normal build must use tools already available in the build environment and
must not install dependencies or access the network. The generated IR records
the extractor version, relevant toolchain versions, extraction mode, and
diagnostics. These inputs also form part of any extraction cache key.

Reproducibility means that the same source repository contents, configuration,
extractor versions, toolchains, and declared environment inputs produce the same
output. Polydoc cannot make an introspected package deterministic when the
package itself is not deterministic.

### Documentation IR

All extractors produce a common, schema-versioned IR.

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
  execution

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
for prose, code, parameter and return sections, admonitions, examples, and
semantic references. Extractors should retain source-format provenance and raw
source where it is useful for diagnostics, but the renderer consumes the
structured form.

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

API reference documentation is only one part of the site.

A workspace should also support any number of authored content collections:

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

The initial implementation supports Polydoc Markdown as its authored format.
Content adapters may later translate other formats into the same structured
document IR, but they must not supply HTML or bypass the common renderer.
Compatibility with arbitrary Sphinx, pkgdown, Documenter.jl, R Markdown, MyST,
or mdsvex extensions is not implied. Unsupported directives and embedded
components produce visible diagnostics.

Normal builds do not execute authored examples or notebook cells. The initial
implementation consumes code blocks, checked-in outputs, and checked-in assets.
A later explicit execution mode may use declared tools and environments, but
its execution mode, toolchain, and inputs must be recorded in provenance and
cache keys just like runtime API introspection.

Authored pages and generated API pages participate in the same navigation, link
resolution, and search index.

--------------------------------------------------------------------------------

## Workspace configuration

A workspace has one root configuration file. It may live in a dedicated
documentation repository or in any one of the source repositories:

```text
polydoc.toml
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
format = "markdown"
execution = "never"
```

Repository paths may point outside the directory containing `polydoc.toml`.
Package paths are relative to their repository roots; metadata and
extraction-target paths are relative to their package roots; content paths are
relative to their repository roots. All must remain within their declared
repository after normalization. This allows explicit sibling checkouts without
making an arbitrary relative path an undeclared source root.

The repository URL identifies the canonical source origin. An optional source
link template controls forge-specific revision, path, and line URLs; Polydoc may
infer standard templates for known forges. Polydoc records a revision and a
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
package's extracted version and declared relationships. A multi-repository
snapshot is coherent only when its binding and dependency constraints match the
versions represented by the supplied sources. `polydoc check` should diagnose
known mismatches but must not resolve, install, or update dependencies.

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

Polydoc does not attempt dependency resolution. It reports inconsistent or
unknown relationships when enough information is available and otherwise
retains them as snapshot metadata.

--------------------------------------------------------------------------------

## Cross-package relationships

Polydoc should support relationships between equivalent or related APIs across
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

Authored Markdown should support package-qualified semantic references:

```text
[`pyfoo::foo.FooModel.fit`]
```

An unqualified shorthand such as ``[`FooModel.fit`]`` may be accepted when it
resolves unambiguously in the current package or workspace. Ambiguous
references are errors reported by `polydoc check`.

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
- API signatures;
- source links;
- breadcrumbs;
- search;
- responsive design.

Language-specific presentation should be implemented through structured renderer
components rather than separate themes.

For example, a Python class page and an R generic-function page may use different
layouts while clearly belonging to the same visual system.

--------------------------------------------------------------------------------

## CLI

The initial CLI should remain small:

```text
polydoc build
polydoc serve
polydoc check
```

Potential additional commands:

```text
polydoc init
polydoc extract
```

`build` should perform extraction, validation, site construction, and rendering.

`check` should validate configuration, source roots, unresolved references,
duplicate identifiers, missing package metadata, incompatible package
relationships, unsupported content constructs, and similar documentation
problems without producing a site.

--------------------------------------------------------------------------------

## Extensibility

API ecosystem support should be modular.

The core should define an extractor interface conceptually similar to:

```text
Extractor
  ecosystem()
  requirements() -> ToolchainRequirement[]
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

--------------------------------------------------------------------------------

## Implementation strategy

The initial implementation should be a vertical slice through one representative
workspace containing related Python and R packages in separate source
repositories. A sensible order is:

1. Define a small multi-repository acceptance corpus containing Python functions
   and classes, public re-exports and type stubs, a native-extension stub, R
   functions and S3 methods, authored Markdown, unsupported content directives,
   and several equivalent and analogous APIs.
2. Spike both extractors against that corpus to discover what their native tools
   expose and where information is lost.
3. Define the repository, package, extraction-target, content-collection, and
   relationship models, together with the structured IR, stable item IDs, and
   conceptual API groups, from the observed data.
4. Implement the Python and R extractors test-first against golden IR fixtures.
5. Implement semantic reference resolution and `polydoc check`, including
   diagnostics for ambiguity, unsupported constructs, incoherent package
   relationships, and unresolved concepts.
6. Render authored pages and both API references in one site.
7. Add package navigation, static workspace search, and the appropriate concept
   switchers.
8. Add end-to-end snapshot tests that verify deterministic output from the
   acceptance workspace.
9. Only then consider historical release assembly or another ecosystem.

Rust is a natural implementation language for the Polydoc core because it
provides a convenient single binary and fits well with parsing, static-site
generation, and concurrent builds. Extractors may invoke Python or R tooling
when required, subject to the declared execution contract.

Rust, Julia, and TypeScript are the next natural public-API extractors for a
core-with-bindings ecosystem. A C extractor is optional: a C ABI may instead be
represented initially by authored reference content and an internal component.

--------------------------------------------------------------------------------

## Non-goals

At least initially, Polydoc is not:

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
  extensions.

The focus is deliberately narrow:

> Generate one coherent documentation snapshot from supplied repositories for
> related Python and R packages, with explicit links between corresponding APIs.
