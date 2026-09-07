# Static-extractor contract

## Outcome

The MVP has two built-in API extractors, identified as `python` and `r`. Both
publish a deterministic manifest containing their version, static extraction
mode, capabilities, parser stack, target, and inputs. Facts emitted by either
extractor carry source provenance separately from this target-level manifest.

This document fixes the logical contract discovered by the extraction spikes.
Milestone 3 will define its serialized IR shape; Milestones 4 and 5 will
implement the adapters. The native parser types and diagnostic messages are not
part of the contract.

## Common contract

### Extractor identity and version

The extractor identifiers are exactly `python` and `r`, matching the values
accepted in an extraction target. A built-in extractor's version is the
Diplodocus crate version from `CARGO_PKG_VERSION`. A parser release or capability
change does not create an ad hoc extractor version: it changes the parser or
capability manifest and therefore changes the complete provenance value and any
cache key derived from it.

### Static mode

Both extractors declare `mode = "static"`. This is the only MVP API-extraction
mode. It means that an extractor:

- runs in the Diplodocus process with Rust-native libraries;
- reads only configured metadata and files reachable from the declared target;
- never starts Python, R, Jupyter, a build backend, or another parser process;
- never imports, installs, builds, sources, attaches, or loads documented code;
- evaluates only an extractor's explicitly supported constant subset; and
- emits a diagnostic when required semantics cannot be established statically,
  without falling back to runtime inspection.

Static extraction may read a referenced source file to resolve a module,
re-export, or documented definition. It may not follow that reference outside
the configured package and repository boundaries or infer another extraction
target.

### Capability manifests

A capability identifier is a stable claim about semantic output, not merely a
claim that a parser accepts some syntax. Every successful target records the
complete, lexicographically sorted set. The two common capabilities are:

| Capability | Guarantee |
| --- | --- |
| `diagnostics.unsupported-visible` | Malformed, dynamic, unsupported, or lossy public input is rejected or represented with a visible diagnostic according to the failure-mode contract; it is not silently discarded. |
| `provenance.source` | Every emitted fact is traceable to one or more repository-relative inputs, with the strongest source range the adapter can prove. |

Adding, removing, or changing the meaning of an identifier is a compatibility
change. It must update the extractor manifest, the relevant acceptance fixture,
and extraction cache keys. Consumers must compare identifiers exactly and must
not infer unlisted capabilities from an ecosystem or parser name.

## Python extractor

### Parser manifest

The Python extractor records every component whose behavior can affect its
semantic output, including AST and range libraries that are not independently
invoked parsers.

| Component | Version | Role and recorded settings |
| --- | --- | --- |
| `pyproject-toml` | `0.13.7` | Parse PEP 621 metadata, PEP 440 versions, and PEP 508 dependencies. No build-backend metadata hooks are permitted. |
| `ruff_python_parser` | `0.0.12` | Parse `.py` and `.pyi`; record `source_type` for each input and the `target_version` selected from the package's declared Python requirement. |
| `ruff_python_ast` | `0.0.12` | Supply the typed syntax consumed by the semantic adapter. |
| `ruff_text_size` | `0.0.12` | Define Ruff's zero-based, half-open UTF-8 byte ranges. |
| `pydocstring` | `0.4.1` | Parse PEP 257 prose and NumPy-style sections; record `style = "numpy"`. |

All five releases are exact pins in `Cargo.toml`. The production adapter must
record the settings above instead of relying on library defaults. In
particular, it must not parse against Ruff's newest supported Python grammar
when the package declares an older target.

### Capability manifest

The Python extractor declares the following sorted capability set, in addition
to no implicit capabilities:

| Capability | Supported semantic subset |
| --- | --- |
| `diagnostics.unsupported-visible` | Common guarantee defined above. |
| `provenance.source` | Common guarantee defined above. |
| `python.declarations` | Modules, functions, classes, methods, properties, constants, fields, parameters, annotations, defaults, return types, supported decorators, and async state. |
| `python.docs.numpy` | PEP 257 prose and supported NumPy Parameters, Returns, Raises, Notes, References, and Examples sections become structured document nodes. |
| `python.exports.static` | Literal `__all__` is authoritative; without it, non-underscore definitions and explicit public imports form the default surface. Computed exports are diagnosed rather than evaluated. |
| `python.metadata.pep621` | Static project name, version, description, Python requirement, dependencies, and typed-package marker. Required dynamic metadata is diagnosed. |
| `python.overloads` | Individual overloads remain addressable and are joined to a callable family. |
| `python.reexports` | Supported relative and absolute imports are resolved to canonical identities without duplicating an item. |
| `python.stubs` | Maintained `.pyi` declarations take field-specific precedence over `.py`, while implementation documentation and provenance remain available; stub-only extension modules are supported. |

The manifest does not claim arbitrary decorator evaluation, arbitrary constant
evaluation, import execution, build-backend metadata, or exact docstring ranges
when decoded text cannot be mapped back to its literal. Those cases follow
[`static-extraction-failure-modes.md`](static-extraction-failure-modes.md).

## R extractor

### Parser manifest

| Component | Version | Role and recorded settings |
| --- | --- | --- |
| `arity-parser` | `0.6.0` | Parse `DESCRIPTION`, `NAMESPACE`, and maintained R source; record the selected grammar as `dcf`, `namespace`, or `r` for each input. Conditional namespace expressions and R calls remain unevaluated. |
| `rd-source` | `0.4.0` | Parse checked-in `Rd` bytes and provide native diagnostics. Dynamic markup remains unresolved. |
| `rd-ast` | `0.4.0` | Interpret supported `Rd` semantics through strict shape-checking views, with default features disabled. Lossy convenience projections are not authoritative. |

All three releases are exact pins in `Cargo.toml`. The extractor records both
`rd-source` and `rd-ast`: parsing can remain unchanged while the semantic view
changes, and either change can affect output.

### Capability manifest

The R extractor declares the following sorted capability set, in addition to no
implicit capabilities:

| Capability | Supported semantic subset |
| --- | --- |
| `diagnostics.unsupported-visible` | Common guarantee defined above. |
| `provenance.source` | Common guarantee defined above. |
| `r.docs.rd` | Checked-in `Rd` names, aliases, usage, arguments, value, description, details, references, examples, and supported inline markup become structured document nodes. Dynamic and unknown markup is not evaluated. |
| `r.metadata.dcf` | Static `DESCRIPTION` package identity, version, title, license, R requirement, Imports, and Suggests constraints. |
| `r.namespace.static` | Unconditional supported exports and imports from `NAMESPACE`. Unknown directives and conditional surfaces are diagnosed rather than flattened. |
| `r.s3` | S3 generic and method registrations, definitions, and `Rd` usage are reconciled into addressable items and callable families. |
| `r.source.functions` | Statically named maintained-source functions and their formals, defaults, and source ranges. Computed definitions are diagnosed rather than evaluated. |

The manifest does not claim roxygen generation, package installation, namespace
loading, source evaluation, dynamic `Rd` evaluation, or exact ranges for
successful `Rd` nodes. Until an exact `Rd` source map exists, those nodes carry
file-level provenance and an incomplete-source-location diagnostic.

## Provenance fields

The following records define logical data, not the final serialization syntax.
They are kept separate so target reproducibility is not confused with the
source evidence for an individual field or document node.

### Target-level extraction provenance

| Field | Required value |
| --- | --- |
| `extractor` | The extractor `id` (`python` or `r`) and its Diplodocus `version`. |
| `mode` | Exactly `static`. |
| `capabilities` | The extractor's complete sorted capability identifiers from this document. |
| `parsers` | A sorted list of component `name`, exact `version`, semantic `role`, and output-affecting `settings` from the relevant parser manifest. |
| `target` | The configured `repository_id`, `package_id`, `target_id`, normalized package-relative `path`, and target `role`. |
| `inputs` | Every file that contributed to the target, each with `repository_id`, normalized repository-relative `path`, input `kind`, content `fingerprint`, and the `parser` component or components used. |

The Python input kinds are `package-metadata`, `python-source`, `python-stub`,
and `typed-marker`. The R input kinds are `package-metadata`, `namespace`,
`r-source`, and `rd`. Inputs, parser records, and capability identifiers are
sorted before serialization. The fingerprint representation and hash algorithm
belong to the schema and cache-key work in Milestone 3; the field is mandatory
here so an extraction result cannot claim provenance without naming all content
that determined it.

### Fact-level source provenance

Every metadata value, visibility decision, item field, signature node,
documentation node, and diagnostic has one or more source records containing:

| Field | Required value |
| --- | --- |
| `repository_id` | ID of the configured repository that owns the source. |
| `path` | Forward-slash-separated path relative to that repository; never an absolute checkout path. |
| `range` | Optional zero-based, half-open UTF-8 byte range. Absence means only file-level provenance is proven and requires the applicable incomplete-location diagnostic. |
| `role` | One of `metadata`, `export`, `registration`, `definition`, `signature`, `documentation`, or `diagnostic`. |
| `parser` | Component identity or identities from the target's `parsers` manifest that produced the fact. It is absent only for direct observations such as `py.typed` file presence. |

Provenance is attached at the narrowest modeled value. An item-level source
location must not erase distinct origins: a Python signature may come from a
stub while its documentation comes from an implementation, and an R item may
combine a namespace export, an R definition, and `Rd` aliases and prose. When
several sources support one fact, their records are ordered by `role`, `path`,
and `range`.

Parser-native messages belong to diagnostics, not provenance. Local repository
roots, timestamps, process IDs, host names, Python or R runtime versions, and
undeclared environment state are forbidden because static extraction neither
uses them nor can serialize them portably.

## Evidence

The selected Python stack and its represented surface are recorded in
[`python-static-extraction.md`](python-static-extraction.md). The executable
probes in
[`tests/python_extraction_spike.rs`](../../tests/python_extraction_spike.rs) and
[`tests/r_extraction_spike.rs`](../../tests/r_extraction_spike.rs) verify the
parser modes, semantic inputs, source ranges, and no-evaluation boundary. The
diagnostic and provenance fallbacks are fixed in
[`static-extraction-failure-modes.md`](static-extraction-failure-modes.md).
