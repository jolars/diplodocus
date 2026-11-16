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

Capabilities describe supported semantics. Do not infer full native discovery
from the presence of a runtime or from the legacy extraction `mode` field:
Julia's source manifest still says `static`, with separate `JuliaRuntime`
evidence for completion. Doxygen also retains its existing `static` manifest
while invoking a native tool. Migrating these records requires an explicit
compatibility decision in a later change.
R command discovery uses `native`, with package-level `RRuntime` observations
and, when enabled, completed `RDocumentation` generation evidence. The public
source extractor retains `static` for supplemental analysis.

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
`.diplodocus/` location keeps storage outside those producer inputs. Low-level
snapshot publication and static source inspection remain independent of native
loading.

| Command | Discovery and execution | Publication |
| --- | --- | --- |
| `check` | Python and Julia source adapters; native R, Rust, and Doxygen extraction. R may run roxygen2 and load the verified checkout. No documentation examples, knitr articles, or Jupyter sessions. | Diagnostics only. Native tools can write caches, generated documentation, or configured side effects. |
| `extract` | Shared API discovery plus Julia runtime completion, verified package loading, API doctests, Rd examples, native authored-document execution, and configured QMD execution. | Completed, validated snapshot with execution evidence and retained assets. |
| `build` | The same extraction operation, followed by snapshot generation. | Snapshot, then site. A generation failure can leave the newer completed snapshot while preserving the previous site. |
| `serve` | The same build operation initially and after watched changes. The current watcher rebuilds the workspace. | Last successful served generation survives a failed rebuild. |
| `generate` | Read a completed snapshot. No producer checkouts, configuration, extraction tools, or language runtimes. | Validated site or fragments. |

Native preparation, generation, extraction, and examples are distinct activities.
R `check` uses the same documentation generation and native discovery as
extraction. A successful `check` does not claim example execution succeeded.
`extract`, `build`, and `serve` must not use different API discovery policies.

Failures, changed inputs, cancellation, unsupported output, and cleanup errors
stop the operation. Successful execution evidence and validated assets must be
retained before publication. Failed extraction preserves the previous snapshot;
failed generation preserves the previous site. Publication cannot undo package
code's side effects or roll back roxygen's checkout writes.

## Capability inventory and migration acceptance

| Adapter | Prepared environment | Current capabilities and evidence | Native migration gaps |
| --- | --- | --- | --- |
| Python | Python and prepared imports/extensions for extraction commands. Source-only checks need no Python interpreter. | Static PEP 621 metadata, maintained stubs, reexports, overloads, supported decorators, structured NumPy docstrings, and source maps. Native helpers verify the checkout and execute doctests. The Python source, surface, docstring, public API, and native example suites cover these features. | Import-time functions and arbitrary generated bindings are not discovered. Native loading does not replace static metadata or declaration rules. A supported Sphinx/autodoc exporter and extension policy remain future work. |
| R | R, pkgload, jsonlite, dependencies, and prebuilt extensions for native discovery; roxygen2 for generated documentation; evaluate for examples and knitr for R Markdown. | Package-level auto selection or explicit roxygen2/Rd mode, in-checkout generation, native exports/formals and S3 evidence, generated declarations, Rd topics, aliases, shared help, and supplemental source evidence. Loading verifies checkout identity and extension bytes. | Package-level incremental invalidation and generation reuse remain step 3. Installed-package loading is rejected; custom roclets require declared supplemental inputs and generated outputs. |
| Julia | Julia and a prepared project for extraction commands; source-only checks need no Julia runtime. | Static includes, exports, aliases, methods, signatures, docstrings, and source maps. Native loading validates the checkout and completes supported generated types and fields. API doctests and Documenter examples retain execution evidence. Julia extraction, runtime, and discovery suites cover these boundaries. | Generated functions, generated methods, and runtime-attached documentation are not generally discovered. `check` retains static diagnostics for generated declarations. A supported documentation/method exporter remains future work. |
| Rust | Prepared `nightly-2026-09-28` toolchain, Cargo.lock, offline dependencies, host target, and explicit library entry. | Native rustdoc JSON format 61 supplies public declarations, macro-generated APIs, reexports, traits, implementations, signatures, links, and source evidence. Native doctests retain compilation, expected errors, and skips. Rust extraction and native runner tests cover the integration. | No replacement planned. Preserve compiler authority and feature selection. Cross-compilation and binary targets remain unsupported. |
| C++ | Doxygen on PATH, an explicit Doxyfile, and prepared generated headers/configuration. No library compilation required. | Native XML supplies namespaces, types, templates, overloads, signatures, references, and source evidence. Doxygen diagnostics and source inventory checks remain effective. Doxygen extraction tests cover extraction and detached generation. | No replacement planned. Standalone Doxygen pages, groups, diagrams, and executable examples are outside the imported API surface. |

The inventory also exposes stale parser-version literals in current provenance:
Legacy R source records use arity-parser `0.6.1` while Cargo selects `0.8.0`, and Julia records toml
`1.1.8` while Cargo selects `1.2.1`. Correcting stale literals and binding them to build-selected versions is a follow-up before
using this evidence for extraction caching. Benchmark reports include direct
runtime version probes separately from verbatim snapshot tool records. Language
IDs in those records can denote the built-in extractor version, not the runtime.

The `native_adapter_contract` suite adds two deliberately unsupported packages.
`generated-python` creates a documented function through `exec`; Python exposes
its signature and docstring, while static extraction reports
`python-unresolved-reexport`. `generated-julia` creates a documented method with a
macro; Julia exposes the callable and attached documentation, while extraction
reports `julia-unresolved-definition`. These cases pin visible gaps without
silently importing a different interpretation. Existing Julia generated-type
tests remain positive acceptance cases.

The same suite compares canonical snapshots from `extract`, `build`, and
`serve`, confirms that Python `check` neither loads the package nor runs its
examples, and verifies that reporting leaves portable evidence unchanged.
Language-specific identity, signature, reference, source-map, and relocation
tests remain the migration acceptance baseline.

Qualpal's public headers and qualpalr's maintained source, Rd topics, shared
topics, and S3 methods provide real-package acceptance inputs. The developer
benchmark uses focused API copies with prebuilt extensions. It excludes
qualpalr's automatically discovered vignettes and Qualpal's standalone Doxygen
guides explicitly. The complete shared-site recipe, authored guide migration,
and cross-repository bundle workflow remain separate roadmap work. Known
qualpalr vignette HTML incompatibilities remain full-site gaps.

R generation currently runs on each command attempt rather than reusing output
from a persistent cache. The watcher acknowledges only observed generator
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
