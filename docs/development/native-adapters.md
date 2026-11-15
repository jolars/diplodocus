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

## Commands and prerequisites

The caller supplies checkouts, runtimes, dependencies, kernels, and compiled
extensions. Diplodocus does not install them. Rust native documentation commands
can compile dependencies and run build scripts with the prepared nightly
toolchain, using locked, offline Cargo commands. Doxygen can run configured
filters. Package code and examples use the caller's privileges.

| Command | Discovery and execution | Publication |
| --- | --- | --- |
| `check` | Current Python, R, and Julia source adapters; native Rust and Doxygen extraction. No documentation examples, knitr articles, or Jupyter sessions. R library discovery may probe installed metadata without loading the documented package. | Diagnostics only. Native tools can write caches or have configured side effects. |
| `extract` | Shared API discovery plus Julia runtime completion, verified package loading, API doctests, Rd examples, native authored-document execution, and configured QMD execution. | Completed, validated snapshot with execution evidence and retained assets. |
| `build` | The same extraction operation, followed by snapshot generation. | Snapshot, then site. A generation failure can leave the newer completed snapshot while preserving the previous site. |
| `serve` | The same build operation initially and after watched changes. The current watcher rebuilds the workspace. | Last successful served generation survives a failed rebuild. |
| `generate` | Read a completed snapshot. No producer checkouts, configuration, extraction tools, or language runtimes. | Validated site or fragments. |

Native preparation, generation, extraction, and examples are distinct activities.
`check` may load packages or invoke additional native tools under the future
contract, but changing its present prerequisites is part of the language
migration. A successful `check` does not claim example execution succeeded.
`extract`, `build`, and `serve` must not use different API discovery policies.

Failures, changed inputs, cancellation, unsupported output, and cleanup errors
stop the operation. Successful execution evidence and validated assets must be
retained before publication. Failed extraction preserves the previous snapshot;
failed generation preserves the previous site. Publication cannot undo package
code's side effects. These guarantees also apply when roxygen generation joins
the pipeline in the next roadmap step.

## Capability inventory and migration acceptance

| Adapter | Prepared environment | Current capabilities and evidence | Native migration gaps |
| --- | --- | --- | --- |
| Python | Python and prepared imports/extensions for extraction commands. Source-only checks need no Python interpreter. | Static PEP 621 metadata, maintained stubs, reexports, overloads, supported decorators, structured NumPy docstrings, and source maps. Native helpers verify the checkout and execute doctests. The Python source, surface, docstring, public API, and native example suites cover these features. | Import-time functions and arbitrary generated bindings are not discovered. Native loading does not replace static metadata or declaration rules. A supported Sphinx/autodoc exporter and extension policy remain future work. |
| R | R for installed-library probing; pkgload, evaluate, dependencies, and prebuilt extensions for examples; knitr for R Markdown. | Maintained DESCRIPTION, NAMESPACE, R definitions, Rd topics, aliases, S3 relationships, shared help, and source evidence. Native loading verifies the checkout and extension bytes. R extraction, help-topic, native example, and article suites cover semantics and retained assets. | No roxygen invocation. Generated declarations must still satisfy supported static rules. Runtime exports/formals are not the API authority. Generation strategy, roclets, output placement, stale-output rejection, and watch-loop prevention belong to step 2. |
| Julia | Julia and a prepared project for extraction commands; source-only checks need no Julia runtime. | Static includes, exports, aliases, methods, signatures, docstrings, and source maps. Native loading validates the checkout and completes supported generated types and fields. API doctests and Documenter examples retain execution evidence. Julia extraction, runtime, and discovery suites cover these boundaries. | Generated functions, generated methods, and runtime-attached documentation are not generally discovered. `check` retains static diagnostics for generated declarations. A supported documentation/method exporter remains future work. |
| Rust | Prepared `nightly-2026-09-28` toolchain, Cargo.lock, offline dependencies, host target, and explicit library entry. | Native rustdoc JSON format 61 supplies public declarations, macro-generated APIs, reexports, traits, implementations, signatures, links, and source evidence. Native doctests retain compilation, expected errors, and skips. Rust extraction and native runner tests cover the integration. | No replacement planned. Preserve compiler authority and feature selection. Cross-compilation and binary targets remain unsupported. |
| C++ | Doxygen on PATH, an explicit Doxyfile, and prepared generated headers/configuration. No library compilation required. | Native XML supplies namespaces, types, templates, overloads, signatures, references, and source evidence. Doxygen diagnostics and source inventory checks remain effective. Doxygen extraction tests cover extraction and detached generation. | No replacement planned. Standalone Doxygen pages, groups, diagrams, and executable examples are outside the imported API surface. |

The inventory also exposes stale parser-version literals in current provenance:
R records arity-parser `0.6.1` while Cargo selects `0.8.0`, and Julia records toml
`1.1.8` while Cargo selects `1.2.1`. Normalization preserves these existing records.
Correcting them and binding them to build-selected versions is a follow-up before
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

See the [pipeline benchmark](pipeline-baseline.md) for reproducible measurements
and the [design](design.md) for the surrounding pipeline.
