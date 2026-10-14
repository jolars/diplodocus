# 0003: Extract in a prepared environment and execute examples

Status: Accepted; partially implemented. Date: 2026-10-12.

## Context

A package documentation build can require the same working environment as the
package itself. Requiring extraction without that environment forces Diplodocus
to reconstruct runtime semantics and leaves executable examples unchecked. R
extraction already requires R and installed dependency metadata, while Python
and Julia extraction currently avoid their runtimes.

This decision supersedes the runtime prohibition in
[0001](0001-static-extraction.md) and the opt-in execution and inert API-example
rules in [0002](0002-authored-content.md). It preserves the shared document IR,
source attribution, validated output boundary, and portable snapshot
architecture.

## Decision

### The caller prepares the package environment

Extraction requires the package's language runtime, its package dependencies,
and the additional dependencies needed by its documentation and examples. The
documented package must be usable from the supplied checkout, including compiled
extensions and other build artifacts where necessary. Loading an unrelated
installed release does not satisfy this prerequisite.

The caller chooses and prepares the environment before invoking Diplodocus. This
includes project activation, library paths, documentation extras, and any
execution engine or kernel required by the selected adapter. Optional features
used by documentation need their dependencies; unrelated optional features do
not become requirements merely because they exist.

Diplodocus does not install packages or kernels, resolve dependencies, build the
documented package, or fetch repositories. An unavailable runtime, dependency,
or required build artifact is an extraction error. Diagnose a detected mismatch
between the loaded package and the supplied checkout rather than silently
documenting another version.

### Extraction may use the runtime

Extractors may load the documented package and its dependencies, inspect runtime
objects, evaluate documentation, and use native language tools to establish
metadata and API semantics. Runtime assistance is part of ordinary extraction,
not a separately authorized fallback. This permission does not require replacing
source parsers when they already provide the needed facts.

Maintain source analysis for documentation structure, declared signatures,
stubs, identities, and source locations where useful. Distinguish facts observed
at runtime from facts derived from source, and diagnose conflicts instead of
silently selecting one. A runtime observation must not acquire an invented
source range. Each adapter defines its reconciliation and supported semantic
subset; runtime availability does not promise complete language coverage.

### Executable examples are required

`extract`, including extraction invoked by `build` or `serve`, executes
recognized executable examples in API documentation and authored pages. No
workspace-wide execution opt-in is required, and there is no successful build
mode that silently leaves executable examples unchecked. This requirement covers
existing examples; it does not require every API item to have an example.

Illustrative code remains distinguishable from executable examples through
explicit source syntax. An ordinary GFM code fence remains display code.
Examples recognized by a language adapter, such as Python doctest transcripts, R
Rd examples, Julia doctests, and QMD executable cells, participate in execution.
Unsupported executable syntax or an unavailable execution engine produces an
error rather than silently becoming display code.

Adapters preserve each format's execution semantics. Sequential examples in one
document share a session where that format requires state; separate documents
use fresh sessions. Setup runs before the examples that depend on it. When the
format supplies expected output, execution verifies that expectation under its
documented comparison rules. Merely running the code is insufficient.

Expected errors and conditional skips must be explicit, supported by the
adapter, and recorded with their source and outcome. A skip records its reason.
An R `\dontrun` region remains explicitly illustrative; `\donttest` alone does
not exempt an example from a documentation build. Missing dependencies or an
unsupported language cannot serve as successful skip reasons. Presentation
options that hide source or output do not disable execution.

### Failures prevent publication

Unexpected language errors, output mismatches, missing prerequisites, timeouts,
invalid outputs, and cleanup failures prevent publication of the new snapshot. A
failed extraction leaves the previous snapshot and site intact, and `serve`
keeps the last successful site available while reporting the failure.

Package loading, hooks, and examples run with the privileges of the user or CI
job. Invoking extraction authorizes that execution. Diplodocus does not sandbox
package code; filesystem changes and network access by that code are outside the
snapshot transaction. Atomic publication cannot undo those side effects. The
output sanitizer continues to protect generated HTML and assets.

### Snapshots contain the completed results

Snapshots retain example outcomes, validated outputs and generated assets,
source attribution, and evidence identifying the runtime, loaded package,
relevant dependencies, adapter, execution policy, and declared environment
inputs. Runtime and dependency observations must describe what actually ran,
without leaking local checkout or library paths into portable records.

A cache may reuse a complete successful execution only when its source,
effective options, runtime and package environment, and validation policy still
match. Reuse records its origin and revalidates outputs and assets. Missing
prerequisites cannot be concealed by a cache hit. Runtime-assisted facts and
example execution need appropriate cache identities; the current QMD cache
format does not automatically provide either for API examples.

Recording environment evidence does not make arbitrary package code
deterministic. Reproducibility depends on controlling randomness, time, network
access, and other external state. A snapshot preserves the results obtained.

`generate` validates and renders a completed snapshot without source checkouts,
language runtimes, package dependencies, or execution engines. It never loads
packages or reruns examples.

`check` remains a source-validation command. It does not load package code or
execute examples, and it does not prove that extraction will succeed. It may
retain bounded runtime discovery, such as the current R library probe, that does
not load package code. Runtime-dependent semantics and example outcomes are
verified during extraction.

## Implementation transition

This decision changes the intended contract, not the current binary. Current
source adapters still analyze declarations statically, and Julia extraction now
completes unresolved exported types through native reflection. Extraction executes R Rd
examples and Python and Julia API doctests in the prepared checkout environment.
QMD execution still requires the existing explicit configuration.
The guides and language-specific implementation references describe that behavior until the
corresponding implementation changes land.

Implementation must update configuration defaults and execution selection,
language-specific example preparation and comparison, package loading and
source/runtime reconciliation, provenance, caches, and snapshot validation.
Choose concrete runtime helpers and any wire-format or policy-version changes as
part of that work, rather than claiming new capabilities in existing static
manifests.

Revise acceptance fixtures to provide working packages and compiled extensions.
Retain source-only fixtures as parser tests, but do not use them as evidence
that a full extraction can succeed without its prerequisites. Add command-level
coverage for default example execution, expected-output mismatches, missing
dependencies, explicit skips, package identity, failure preservation, and
generation after removing the source and runtime environment. The [status
page](../../TODO.md) tracks this work.
