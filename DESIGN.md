# Polydoc: Design

## Purpose

Polydoc is a documentation generator for polyglot software projects, especially
monorepos containing multiple related packages.

Its central goal is to provide **one coherent documentation website** for
packages implemented in different languages, rather than composing sites
produced independently by tools such as rustdoc, pkgdown, Sphinx, or
Documenter.jl.

Typical projects include:

- Python and R packages that expose the same statistical library;
- a core library with bindings for several programming languages;
- several independently released packages in one monorepo;
- packages written in different languages but belonging to the same software
  project.

Polydoc should make these appear as parts of one documentation system, with
common navigation, styling, search, URLs, and page structure.

### Initial scope

The first release targets workspaces containing related Python and R packages.
It should prove the complete workflow for those two ecosystems before adding
other languages. Rust and Julia are natural candidates for later extractors,
but are not part of the initial scope.

The initial product is one coherent snapshot of the documentation in the
current source tree. Historical release assembly is a later concern.

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

### Monorepo first

A workspace may contain any number and combination of packages:

```text
project/
├── python/
├── R/
├── docs/
└── polydoc.toml
```

Packages need not share a language or release cycle.

### Static output

The primary output is a self-contained static website suitable for GitHub Pages,
Cloudflare Pages, Netlify, or any ordinary HTTP server.

### Reproducible builds

The same source tree and configuration should produce the same documentation
output, apart from explicitly non-reproducible metadata.

Polydoc should not perform implicit network access during normal builds.

--------------------------------------------------------------------------------

## Architecture

The system consists of four major layers:

```text
source packages
      │
      ▼
language extractors
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

### Language extractors

Each supported ecosystem has an extractor that translates package metadata,
APIs, and documentation into Polydoc's intermediate representation.

Initial extractors:

```text
Python
R
```

Possible later extractors include Rust and Julia.

Extractors should use native semantic infrastructure where useful.

For the initial extractors:

- Python should use static source analysis and package metadata by default.
  Import-based introspection is an explicit optional mode.
- R should read `DESCRIPTION` and `NAMESPACE` metadata and consume parsed `Rd`
  documentation without loading the package by default. Runtime introspection
  is an explicit optional mode.

The extractor boundary should remain independent from the renderer.

Conceptually:

```text
polydoc extract python ./python
         │
         ▼
      package IR
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

Reproducibility means that the same source, configuration, extractor versions,
toolchains, and declared environment inputs produce the same output. Polydoc
cannot make an introspected package deterministic when the package itself is
not deterministic.

### Documentation IR

All extractors produce a common, schema-versioned IR.

A simplified model is:

```text
Workspace
  Package[]
  Page[]
  Concept[]

Package
  id
  slug
  name
  language
  version
  source
  items[]

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

Every item ID is scoped by a stable package ID and assigned by its language
extractor. An item ID must distinguish overloaded functions, R methods and
generics, aliases, and other entities that may share a qualified name. It should
remain stable while the corresponding public API remains unchanged.

Item IDs are not URLs. The site model derives URLs from package slugs and item
metadata, which permits redirects and layout changes without changing semantic
references.

### Authored documentation

API reference documentation is only one part of the site.

A workspace should also support authored documentation:

```text
docs/
├── index.md
├── getting-started.md
├── concepts.md
└── examples.md
```

Markdown is the default authored format.

Authored pages and generated API pages participate in the same navigation, link
resolution, and search index.

--------------------------------------------------------------------------------

## Workspace configuration

A repository contains a root configuration file:

```text
polydoc.toml
```

For example:

```toml
[project]
name = "Foo"
repository = "https://github.com/example/foo"

[[package]]
id = "pyfoo"
name = "Foo for Python"
slug = "python"
language = "python"
path = "python"

[[package]]
id = "rfoo"
name = "Foo for R"
slug = "r"
language = "r"
path = "R"

[docs]
path = "docs"
```

The package `id` is the stable identity used by references and relationships.
The `slug` controls its URL and must be unique within the site. Neither is
derived from the package's language, so a workspace may contain several Python
or R packages.

Configuration should be explicit and small.

Automatic package discovery may be added later, but the configuration file
remains authoritative.

--------------------------------------------------------------------------------

## Build snapshots and versions

A workspace contains multiple packages, and their versions need not match. A
normal build represents one snapshot of the current source tree and records the
version of each package independently.

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

Historical documentation requires assembling snapshots built from different
source revisions. That operation is outside the initial `build` command. A
later release assembler may consume existing snapshot artifacts or explicit
checkouts, but it must not make ordinary builds depend on implicit version
control operations or network access.

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
set of languages or independently by each language adapter.

The interface should identify each package's language and provide a package
switcher where appropriate. Pages belonging to a conceptual API group should
also provide a direct switcher between that concept's implementations.

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
members = [
  { package = "pyfoo", item = "foo.FooModel.fit" },
  { package = "rfoo", item = "fit.foo_model" },
]
```

A concept may contain any number of members from any combination of packages,
including several packages written in the same language. Members resolve to
semantic item IDs during validation. Qualified names are accepted as authoring
conveniences only when they resolve unambiguously.

Concept pages and member pages should expose the relationship prominently, for
example through a "Same API in" switcher. Automatic inference may be considered
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

Search results should identify both package and language.

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

`check` should validate configuration, unresolved references, duplicate
identifiers, missing package metadata, and similar documentation problems
without producing a site.

--------------------------------------------------------------------------------

## Extensibility

Language support should be modular.

The core should define an extractor interface conceptually similar to:

```text
Extractor
  language()
  requirements() -> ToolchainRequirement[]
  capabilities() -> ExtractionCapabilities
  extract(context, package) -> ExtractionResult

ExtractionResult
  package_ir
  diagnostics[]
  provenance
```

Configuration is authoritative, so automatic language detection is not part of
the initial interface. Language adapters should not have access to rendering
internals.

Initially, adapters can live in the main repository. A stable external plugin
API is unnecessary until the internal IR and extractor API have matured.

--------------------------------------------------------------------------------

## Implementation strategy

The initial implementation should be a vertical slice through one representative
workspace containing related Python and R packages. A sensible order is:

1. Define a small acceptance corpus containing Python functions and classes, R
   functions and S3 methods, authored Markdown, and several paired APIs.
2. Spike both extractors against that corpus to discover what their native tools
   expose and where information is lost.
3. Define the package model, structured IR, stable item IDs, and conceptual API
   groups from the observed data.
4. Implement the Python and R extractors test-first against golden IR fixtures.
5. Implement semantic reference resolution and `polydoc check`, including
   diagnostics for ambiguity, unsupported constructs, and unresolved concepts.
6. Render authored pages and both API references in one site.
7. Add package navigation, static workspace search, and the "Same API in"
   switcher.
8. Add end-to-end snapshot tests that verify deterministic output from the
   acceptance workspace.
9. Only then consider historical release assembly or another language.

Rust is a natural implementation language for the Polydoc core because it
provides a convenient single binary and fits well with parsing, static-site
generation, and concurrent builds. Extractors may invoke Python or R tooling
when required, subject to the declared execution contract.

--------------------------------------------------------------------------------

## Non-goals

At least initially, Polydoc is not:

- a general-purpose static site generator;
- a replacement for Markdown authoring;
- a package manager;
- a build system for the packages themselves;
- a historical documentation release assembler;
- a hosted documentation service;
- an IDE documentation engine;
- a universal source-code parser;
- a compatibility layer for existing Sphinx/pkgdown/Documenter themes or
  extensions.

The focus is deliberately narrow:

> Generate one coherent documentation snapshot for related Python and R packages,
> with explicit links between corresponding APIs.
