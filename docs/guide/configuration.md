# Workspace configuration

The root `diplodocus.toml` is an authored-only example of the intended MVP
configuration. The library parses the `project`, `repository`, `package`,
`content`, `concept`, and `relationship` sections into typed declarations:

```rust
let config = diplodocus::configuration::load_configuration("diplodocus.toml")?;
```

Use `configuration::parse_configuration` to parse a TOML string. Both entry
points reject unknown fields, missing required fields, incorrect types, and
unsupported enum values. They also validate each collection's execution settings
together. Load errors include the configuration path and the
underlying read or parse error; TOML errors retain source ranges when available.
Omitted collections are empty, package kind defaults to `package`, visibility
defaults to `public`, and execution mode defaults to `never`.

Repositories and packages are documented only when explicitly declared; loading
a configuration never discovers them from nearby source files. Documentation-only
workspaces may omit packages. Every declared package requires a `targets` list:
`targets = []` explicitly declares no API extraction.

Execution requires `format = "qmd"`, `mode = "execute"`, `engine = "jupyter"`,
and an explicit kernel selector. Kernel selectors contain only ASCII letters,
digits, `-`, `.`, or `_`; empty names, `.` and `..`, and paths are rejected.
The `never` mode accepts no engine, kernel, or nonempty environment-input list.
Omitting the mode never enables execution, even when a kernel is specified.

Environment inputs must declare individual repository-relative files. Parsing
rejects empty paths, absolute paths, repository escapes, directory references,
glob patterns, and duplicate paths after lexical normalization. It preserves
the declared spelling, including the kernel selector's case, without reading
inputs or discovering kernels. Programmatically modified collections can repeat
these checks with `ContentConfiguration::validate_execution`.

Use `documents::parse_collection_document(source, &collection)` to parse an
authored page and check its execution declarations against its owning collection.
Invalid collection settings return an error. Document violations appear as
error-severity entries in `DocumentParse::diagnostics`, which callers must inspect
before proceeding. `validation::validate_document_execution` checks an already
parsed document; `documents::parse_authored_document` provides syntax parsing alone.
Authority validation performs no kernel discovery or execution.

In a `never` collection, document `execute: true` and execution selectors produce
one `document-execution-not-authorized` error with related source ranges. Document
selectors are also rejected in authorized collections as
`unsupported-qmd-metadata`. Supported restrictions, such as `execute: false`, and
cell defaults remain in the document without granting collection authority.
YAML merge keys are rejected in document and `execute` mappings. Malformed or
duplicate YAML retains its parser error.

Parsing retains declared paths, owners, concept members, and relationship
endpoints for later validation. Filesystem resolution, file existence and type
checks, symlink containment, identity and relationship validation, general
metadata and cell-option validation, and command integration remain under development.
A parsed configuration alone does not authorize execution.

## Repositories and ownership

Repository paths are relative to the configuration directory and may identify
sibling checkouts. Package paths are relative to their repository, and API
target paths are relative to their package. Authored content and declared
environment inputs are relative to the named repository.

The `project` owner places content in project navigation. A package ID gives a
collection package ownership and that package's reference-resolution context.
Package documentation lives under `/packages/<slug>/`.

## The project site

This configuration declares one repository and two project-owned collections:

| Collection | Source | Mount | Profile | Execution |
|:-----------|:-------|:------|:--------|:----------|
| Guide | `docs/guide` | `guide` | GFM | Never |
| Examples | `docs/examples` | `examples` | QMD | Python |

The guide's omitted execution settings default to `never`. The examples
collection explicitly declares:

```toml
[content.execution]
mode = "execute"
engine = "jupyter"
kernel = "python3"
declared_environment_inputs = ["devenv.lock"]
```

The declared lockfile contributes to provenance and execution-cache keys.
Diplodocus does not interpret it as an installation instruction. One page uses
one kernel session, with its cells run in source order.

## Profiles and assets

GFM collections discover `.md` files. QMD collections discover `.qmd` files and
support executable fences, hashpipe options, and callouts. Unsupported syntax
produces a visible diagnostic. Raw authored HTML is escaped or represented by
an unsupported node.

Keep checked-in assets beside their owning content. Generated figures become
local content-addressed assets, and generated output cannot read assets outside
its declared boundary. Output directories and execution caches are not inputs
to watched builds.

Return to the [overview](index.md) or try the [quick start](quick-start.md).
