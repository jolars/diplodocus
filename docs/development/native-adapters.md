# Native adapter boundary

Diplodocus owns the documentation IR, symbolic cross-language links, snapshots,
site routes, navigation, search, rendering, and preview loop. Language tooling
supplies API semantics. An adapter returns structured records, not another
generator's HTML site. This is the boundary for the
[native extraction roadmap](https://github.com/jolars/diplodocus/blob/main/TODO.md), including adapters that still use
static semantics today.

## Handoff to assembly

All five built-in adapters normalize their output to the internal `AdapterOutput`
and `AdapterEvidence` types. This is an in-process contract, not an external
plugin interface or a new snapshot format. Existing public extractor functions
retain their language-specific, inspectable results.

| Record | Contract |
| --- | --- |
| Target | Package and target identity from extraction provenance must match the configured target. |
| Package metadata and version | Existing `PackageMetadata` variants retain language-specific fields. Assembly detects disagreement between targets and merges R help-link coordinates. |
| API items | Canonical `Item` keys, aliases, overloads, methods, signatures, language records, and child relationships survive normalization. Identity does not depend on routes or native temporary IDs. |
| Documentation | Existing sourced document nodes retain supported prose, raw source, decoded source maps, and inert executable syntax until authorized execution. |
| References | Structured symbolic references retain spelling, labels, and context. Workspace assembly resolves them before site routing. |
| Source evidence | Repository-relative locations and proven ranges remain attached to individual facts. Runtime observations do not acquire invented source declarations. |
| Diagnostics | Unsupported, ambiguous, and malformed input remains visible. Assembly merges diagnostics deterministically and rejects errors before publication. |
| Tool evidence | Existing extraction provenance carries tool versions, parser settings, and capabilities. The contract requires exact versions; current discrepancies are listed below. Native completion evidence remains attached to its original records. |
| Observed inputs | Repository-relative input kinds, fingerprints, and contributing parsers are accessible through the shared evidence. They describe observed reads, not a complete dependency graph or an extraction cache key. |
| Completed execution | Rust doctest evidence accompanies the target handoff. Other native results and assets join the workspace in their existing execution stages. None can be inferred from successful API discovery alone. |

Native process supervision, temporary directories, absolute runtime paths,
discovery roots, cancellation, deadlines, and revalidation owners stay outside
portable output. Doxygen's observation guard survives conversion through the
workspace owner. Inputs are rechecked before accepting later work. Generation
from a completed snapshot requires none of these local owners.

Capabilities describe supported semantics. Julia command discovery records
`native` and includes `JuliaRuntime` evidence for the verified checkout.
Doxygen records its existing `static` mode with Doxygen tool and parser
evidence.
R command discovery uses `native`, with package-level `RRuntime` observations
and, when enabled, completed `RDocumentation` generation evidence.

## Commands and prerequisites

The caller supplies checkouts, runtimes, dependencies, kernels, and compiled
extensions. Diplodocus does not install them. Rust native documentation commands
can compile dependencies and run build scripts with the prepared nightly
toolchain, using locked, offline Cargo commands. Doxygen can run configured
filters. Package code and examples use the caller's privileges.

Native tools retain their own rules for discovering files. Excluding generated storage
from Diplodocus's source inventories does not hide it from package loaders. R
commands reject recognized snapshots in native code, help, or data locations,
and CLI publication rejects new snapshot destinations there. The default
`.diplodocus/` location keeps storage outside those producer inputs.

| Command | Discovery and execution | Publication |
| --- | --- | --- |
| `check` | Native API discovery and reference validation. Julia loads and reflects on its checkout. Python imports its checkout and invokes Sphinx. R may run roxygen2 and load its checkout. Document examples stay inert. | Diagnostics only. Native tools can write caches, generated documentation, or configured side effects. |
| `extract` | Shared API discovery, API doctests, Rd examples, native authored-document execution, and configured QMD execution. | Completed, validated snapshot with execution evidence and retained assets. |
| `build` | The same extraction operation, followed by snapshot generation. | Snapshot, then site. A generation failure can leave the newer completed snapshot while preserving the previous site. |
| `serve` | The same build operation initially and after watched changes, reusing verified package and page results when their inputs match. Workspace references and site output are rebuilt. | Last successful served generation survives a failed rebuild. |
| `generate` | Read a completed snapshot. No producer checkouts, configuration, extraction tools, or language runtimes. | Validated site or fragments. |

The Rust library's `assembly::assemble_workspace()` uses the same native
discovery as `check`, including R checkout loading and configured roxygen2
generation. Both keep documentation examples inert.

Failures, changed inputs, cancellation, unsupported output, and cleanup errors
stop the operation. Successful execution evidence and validated assets must be
retained before publication. Failed extraction preserves the previous snapshot;
failed generation preserves the previous site. Publication cannot undo package
code's side effects or roll back roxygen's checkout writes.

## Capability inventory and migration acceptance

| Adapter | Prepared environment | Current capabilities and evidence | Native migration gaps |
| --- | --- | --- | --- |
| Python | Python >=3.11, a compatible Sphinx >=9.0.4,<10, prepared dependencies, and compiled extensions for all discovery commands. | Native exports, generated declarations, attached prose, signatures, maintained stubs, aliases, overloads, and reStructuredText/NumPy/Google documentation through autodoc/Napoleon. Discovery and doctests retain separate completed evidence and caches. | Built-in Sphinx configuration only; project conf.py, third-party extensions, MyST, and Markdown docstrings remain outside the supported interface. Static PEP 621 metadata remains required. Import failures never select a static fallback. |
| R | R, pkgload, jsonlite, dependencies, and prebuilt extensions for native discovery; roxygen2 for generated documentation; evaluate for examples and knitr for R Markdown. | Package-level auto selection or explicit roxygen2/Rd mode, in-checkout generation, native exports/formals and S3 evidence, generated declarations, Rd topics, aliases, shared help, and supplemental source evidence. Loading verifies checkout identity and extension bytes. Verified generation, discovery, metadata, and execution results have separate persistent cache entries. | Installed-package loading is rejected; custom roclets require declared supplemental inputs and generated outputs. Dependency uncertainty prevents reuse. |
| Julia | Julia 1.10 through 1.13 and a prepared package project for discovery and example execution. | Verified bindings, aliases, method dispatch, generated declarations, attached documentation, and source input evidence. Fatou supplies source attribution and explicitly selected extension implementations. API doctests and Documenter examples retain execution evidence. | External-only reexports and unsupported attachment formats produce diagnostics. Explicit extension source selection covers authored methods. |
| Rust | Prepared `nightly-2026-09-28` toolchain, Cargo.lock, offline dependencies, host target, and explicit library entry. | Native rustdoc JSON format 61 supplies public declarations, macro-generated APIs, reexports, traits, implementations, signatures, links, and source evidence. Native doctests retain compilation, expected errors, and skips. Rust extraction and native runner tests cover the integration. | No replacement planned. Preserve compiler authority and feature selection. Cross-compilation and binary targets remain unsupported. |
| C++ | Doxygen on PATH, an explicit Doxyfile, and prepared generated headers/configuration. No library compilation required. | Native XML supplies namespaces, types, templates, overloads, signatures, references, and source evidence. Doxygen diagnostics and source inventory checks remain effective. Doxygen extraction tests cover extraction and detached generation. | No replacement planned. Standalone Doxygen pages, groups, diagrams, and executable examples are outside the imported API surface. |

Parser versions used by cache identities come from the current executable build
and its selected dependencies. Recorded tool versions alone are insufficient
cache keys: runtime and installed dependency observations must also match.
Benchmark reports include direct runtime version probes separately from verbatim
snapshot tool records. Language IDs in those records can denote the built-in
extractor version, not the runtime.

The `native_adapter_contract` suite includes two generated-API packages.
`generated-python` creates a documented function through `exec`; Python exposes
its signature and docstring, and native Sphinx discovery retains both.
`generated-julia` uses a macro to create a documented method. The native
exporter retains that method, an evaluated alias, and registered prose.
The acceptance test checks the method reference and verifies the doctest
during extraction. The native matrix also exercises generated types and
computed includes on all four supported Julia minors.

The same suite compares canonical snapshots from `extract`, `build`, and
`serve`, confirms that Python `check` loads the package without running its
examples, and verifies that reporting leaves portable evidence unchanged.
Language-specific identity, signature, reference, source-map, and relocation
tests remain the migration acceptance baseline.

Python compatibility checks cover Sphinx 9.0.4 with Python 3.11 and Sphinx
9.1.1 and 9.2.0 with Python 3.14. They exercise generated declarations,
overloads, reStructuredText, NumPy and Google docstrings, module sections,
stub-only documentation, changed exports, inert examples, and import failures.
Run `compute run -- python scripts/test-python-compat.py` in a prepared
environment, or repeat `--profile SPHINX_VERSION=PYTHON` for prepared
interpreters. The script does not install dependencies.

Qualpal's public headers and qualpalr's maintained source, Rd topics, shared
topics, and S3 methods provide real-package acceptance inputs. The developer
benchmark uses focused API copies with prebuilt extensions. It excludes
qualpalr's automatically discovered vignettes and Qualpal's standalone Doxygen
guides explicitly. The complete shared-site recipe, authored guide migration,
and cross-repository bundle workflow remain separate roadmap work. Known
qualpalr vignette HTML incompatibilities remain full-site gaps.

R generation and native discovery can reuse completed results when package and
environment observations still match. Package loading and examples have separate
reuse rules; generated Rd alone does not prove execution remains current.
`incremental.depends-on`, `extra-inputs`, and `environment-variables` describe
additional package dependencies and maintained reads. Native environment probes
and installed dependency observations remain part of verification. Cache hits
retain completed evidence and reconstruct local observations before publication.
The watcher acknowledges only observed generator
writes, including partial output after a failed attempt, and retains maintained
source changes for another attempt. The previous successful snapshot and site
survive generation failures. Roxygen2 supports `>= 7.3.2, < 9`. Compatibility
checks pass with 7.3.2, 8.0.0, and 8.1.1 using both `pkgload` and `source`.
These checks cover generation, native reflection, changed exports and deleted
topics, inert examples, and diagnostic and cleanup handling.

Run `compute run -- python scripts/test-roxygen-compat.py` to check the current
prepared library. Repeat `--profile VERSION=LIBRARY` for each caller-prepared
version profile. The script exercises both supported loaders, native formals,
generated documentation, inert examples, and generation-error rejection. It does
not install versions or change producer checkouts. Retain explicit version
profiles when extending the compatibility matrix.

See the [pipeline benchmark](pipeline-baseline.md) for reproducible measurements
and the [design](design.md) for the surrounding pipeline.
