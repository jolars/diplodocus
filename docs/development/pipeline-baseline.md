# Pipeline baseline

The developer benchmark measures the production preview loop through
`CommandRunner::serve_with`. It records startup until serving readiness and each
source save until a successfully published rebuild. Save times include polling
and debounce. Browser refresh transport and browser rendering are not measured.

## Run a comparison

Build the worker before acquiring exclusive benchmark access:

```sh
cargo build --locked --release --example pipeline_bench
mkdir -p artifacts/native-adapters
cp target/release/examples/pipeline_bench artifacts/native-adapters/baseline
```

Apply the candidate change, build again, and compare both executables in a single
foreground reservation:

```sh
cargo build --locked --release --example pipeline_bench
cp target/release/examples/pipeline_bench artifacts/native-adapters/candidate
python scripts/bench-pipeline.py \
  --worker baseline=artifacts/native-adapters/baseline \
  --worker candidate=artifacts/native-adapters/candidate \
  --output artifacts/native-adapters/comparison.json
```

For the initial boundary refactor, the baseline is the parent implementation
with the measurement instrumentation and worker added. Future comparisons can
use the worker available in both revisions. Archive executable digests, source
revisions, and any instrumentation-only patch with the measurement artifacts.
Keep both executables outside `target/` so cache cleanup cannot remove a worker
partway through the comparison.

The default cases are the four-language monorepo example, Qualpal public
headers, and qualpalr API documentation. Supply different producer checkouts
with `--qualpal` and `--qualpalr`. Both executables must inherit the same prepared
environment. For local qualpalr measurements, its existing
`.devenv/profile/bin` can supply the prepared R wrapper on PATH. The harness
does not create environments, install packages, or compile producer extensions.

Use `--scenario python` for the Python migration profile. It copies the
monorepo's Python package, adds an authored page, and measures authored prose,
API docstring prose, and return-annotation saves. Native Python workers need
Sphinx in the prepared interpreter. Add `--warm-restart` to verify that retained
caches preserve the completed site and snapshot after restarting the worker.

Each executable receives equivalent temporary copies. The Qualpal profile uses
a headers-only Doxyfile. The qualpalr profile selects maintained API inputs,
native artifacts, runtime assets, package metadata, and news, and excludes
vignettes. The original producer checkouts are never edited. Missing tools,
dependencies, incompatible compiled extensions, or failed builds produce
explicit blocked cases rather than substitute packages or successful timings.

An optional `--qualpalr-patch PATH` applies a prepared input correction to each
temporary package before measurement. The report retains the patch and its
digest. The recorded roxygen comparison uses
[`qualpalr-description.patch`](https://github.com/jolars/diplodocus/blob/main/docs/development/benchmarks/qualpalr-description.patch) to separate
a deprecated topic's description from its explicit title. Without that source
correction, roxygen2 rejects the producer's multiparagraph `@title`. Both workers
receive the same correction. If the prepared extension needs an OpenMP library
outside its recorded search paths, expose that library through `LD_LIBRARY_PATH`.
The report records the runtime library path and thread settings.

The shared library must match the selected R source and native interface. A stale
Rcpp-built `src/qualpalr.so` cannot serve the current cpp11 sources, even if the
library loads. Compile the current extension in a temporary package with the
caller's prepared dependencies, then pass `--qualpalr-dll PATH/qualpalr.so`.
For the recorded comparison, preparation used
`pkgbuild::compile_dll(path, force = TRUE, compile_attributes = FALSE, debug = FALSE)`
in that copy, keeping the existing generated cpp11 registration code.
The harness copies it into each temporary package's `src/` after applying any
input patch and before measurement. The report retains its path and SHA-256
digest. The original producer checkout stays unchanged; the harness never
compiles the extension. The DLL override and input patch can be used separately
or together.

The harness performs one unreported warmup, five fresh-workspace startup runs,
and two saves of each mutation kind per startup. That gives ten warm-save
observations per kind. Startup has fresh Diplodocus caches but shares warmed OS
and native runtime caches. It is not a machine-cold boot measurement. Warm saves
reuse the same running server and its workspace caches.

Add `--warm-restart` to measure one restart in each measured workspace after its
save cases, keeping completed caches. The harness verifies that restarting leaves
the completed snapshot fingerprint and every site file unchanged. These samples
use the `warm-startup` kind, separate from startup in a fresh workspace. Updated
workers also emit cache hits and misses; reports aggregate them by action, stage,
and owner. Older workers remain usable and simply have no cache events.

Keep startup and save comparisons separate from this optional restart gate when
the baseline regenerates documentation on every attempt. A fresh generation can
legitimately record different historical inputs after removing a generated
topic, even when the completed site is identical. Validate strict restart parity
for the incremental worker when its completed generation entry is reused.

Mutations cover authored prose, API prose, and signatures. The harness detects
completed roxygen2 generation from the startup snapshot's `RDocumentation`
provenance. For that worker, qualpalr API prose and signature saves edit only
`R/qualpal.R`; the harness requires regenerated Rd and changed rendered output.
An additional export case appends a documented helper and requires both Rd and
NAMESPACE to change. It never patches generated Rd by hand.

Workers without generation retain the original maintained-Rd cases: signature
edits update source and usage together, and `roxygen-gap` requires a roxygen prose
edit to leave Rd and rendered output unchanged. That row measures the old gap,
not successful native preview behavior. The report records each worker's
generation mode and mutation inputs. Initial maintained-input hashes remain
comparable, but native discovery, regenerated output, and provenance can change
snapshot fingerprints. Do not require identical fingerprints across this
migration. Each change is restored before the next mutation category.
`--scenario authored --samples 1 --saves 2` provides a small harness smoke test.

For a focused R comparison, add `--scenario qualpalr --samples 1 --saves 2` to
the command above. The candidate also performs an unmeasured failure/recovery
check through the production serve loop: introduce invalid maintained R source,
await the generation error, verify the previous snapshot and site remain byte
identical, correct the source, and await a successful rebuild. Both states must
settle without a generated-write rebuild loop. The result appears under
`generation_recovery`; its events are excluded from performance summaries.
The candidate must also remain idle after startup and every source save.

For a manual check, use the same focused qualpalr API copy with its prepared
extension and dependencies, and run plain `diplodocus serve`. Change a roxygen
title in `R/qualpal.R` without touching `man/`, then verify the generated Rd and
served title update. Introduce an unmatched parenthesis in the R source, verify
the previous page remains available, and correct it to verify recovery. Browser
refresh is enabled by default; use `--no-live-reload` to disable it. The complete upstream qualpalr
vignettes remain outside this API-only recipe.

## Interpreting timings

Structured Reporter events identify extraction targets and stable phases:
package loading, documentation generation, extraction, native processes,
package metadata, examples, reference resolution, site preparation, rendering,
and publication. Phases close with success or failure, including cancellation.
Existing human-facing progress labels and the normal CLI remain unchanged.

Native helper sidecars separate package loading from API example execution and
Julia reflection. They use monotonic elapsed clocks and live only in private
staging. A native-process duration also includes interpreter startup, loading,
execution, and shutdown. Native subphase durations overlap the host duration;
do not add them together. Empty authored execution stages can have small host
durations without starting a runtime. Native R workers report generation
separately from package loading; workers without roxygen generation have no
generation timing. Historical baseline workers generate on every command
attempt. Incremental workers report `cache-validation` for native environment
probes, dependency fingerprinting, and cache validation. These timings describe
verification work; a hit does not imply a zero-cost build.

Incremental workers also report `source-verification` for each fresh input guard,
`watch-capture` for subscription snapshots, and `watch-poll` for synchronous
polling scans. Poll durations exclude the idle interval. Source verification can
be nested inside other host phases, so these durations also overlap. Use the
per-sample event boundaries when accounting for end-to-end latency.

Reports include raw samples, phase events, input hashes, executable hashes,
source revisions, and observed tool versions. Summary values use the median and
nearest-rank p95. The sample sizes establish a practical local baseline, not a
statistically precise tail distribution. Use measured baseline p95 values as
initial startup and warm-save budgets, and compare future changes in the same
exclusive window. Performance thresholds are not CI assertions.

Timings never enter extraction provenance, semantic records, execution caches,
snapshots, or content fingerprints. The adapter contract and existing portable
execution evidence remain the authority for correctness. Rebuilding the producer
executable can still invalidate existing execution identities through the
existing build-fingerprint policy.

## Initial results

Recorded on an AMD Ryzen 9 7900 (24 logical CPUs), Linux 6.18.60, with optimized
workers in one exclusive measurement session. The runtime versions were
Python 3.14.7, R 4.6.1, Julia 1.12.7, and Doxygen 1.17.0. Rust extraction used the
prepared nightly toolchain described in the adapter contract. The baseline
contains measurement instrumentation; the candidate adds the adapter boundary.

All 230 samples completed. Input hashes and snapshot fingerprints matched
between baseline and candidate for every corresponding sample. No checkout was
modified. The retained [raw measurements](https://github.com/jolars/diplodocus/blob/main/docs/development/benchmarks/native-adapters.json)
include individual host/helper durations, tool records, diagnostics, and input
hashes. Phase rows reference the shared context table and contain elapsed seconds
and host success (null for native subphases). Tool records are deduplicated per
case. The [baseline instrumentation patch](https://github.com/jolars/diplodocus/blob/main/docs/development/benchmarks/native-adapter-instrumentation.patch)
applies to the recorded parent revision. The full event trace and baseline
executable remain in local `artifacts/native-adapters/`.

Times below are **median / p95 in seconds**. `roxygen-gap` measures a source
change whose documentation remains unchanged, not a completed native preview.

| Case | Action | Baseline | Candidate |
| --- | --- | --- | --- |
| monorepo | startup | 10.273 / 10.348 | 10.365 / 10.473 |
| monorepo | authored | 10.195 / 10.244 | 10.382 / 10.544 |
| monorepo | api-prose | 10.175 / 10.247 | 10.455 / 10.659 |
| monorepo | signature | 10.170 / 10.238 | 10.314 / 10.527 |
| qualpal | startup | 0.367 / 0.375 | 0.361 / 0.384 |
| qualpal | authored | 0.632 / 0.654 | 0.631 / 0.643 |
| qualpal | api-prose | 0.631 / 0.645 | 0.635 / 0.647 |
| qualpal | signature | 0.639 / 0.709 | 0.638 / 0.643 |
| qualpalr | startup | 4.781 / 4.806 | 4.839 / 4.875 |
| qualpalr | authored | 4.949 / 5.043 | 5.094 / 5.356 |
| qualpalr | api-prose | 4.947 / 5.029 | 5.068 / 5.223 |
| qualpalr | signature | 4.961 / 5.022 | 5.029 / 5.358 |
| qualpalr | roxygen-gap | 4.943 / 5.206 | 5.027 / 5.355 |

Use these rounded baseline p95 values as the initial local preview budgets:
monorepo startup **10.35 s** and warm saves **10.25 s**; Qualpal startup **0.38 s**
and warm saves **0.71 s**; qualpalr API startup **4.81 s** and maintained-Rd saves
**5.05 s**. Roxygen generation needs its own completed-preview budget measured
with the native worker. These are measured comparison points, not CI gates or performance
improvements delivered by this refactor.

On monorepo authored saves, baseline native-process medians were 1.64 s for
Julia checkout completion, 2.84 s for Julia API examples, 2.61 s for R examples,
and 0.91 s for Python checking/examples. Rendering took 0.12 s and reference
resolution 0.012 s. Qualpalr's native example processes totaled a median 4.30 s,
compared with 0.11 s in rendering. Native-process totals include startup and
other helper overhead; do not interpret them as package-loading time alone.

Candidate monorepo warm-save medians were 1.4–2.8% higher, qualpalr medians were
1.4–3.0% higher, and Qualpal warm-save medians changed by less than 1%. The
monorepo difference was concentrated in native processes. This single ordered
comparison does not establish a causal performance regression or speedup.
An exclusive measurement session leaves CPU frequency, thermal state, and
unrelated system activity uncontrolled. Keep the measured breakdown as the
baseline for native orchestration and package invalidation work.


## Roxygen2 workflow results

Recorded on November 16, 2026, on an Intel Core Ultra 7 155U (14 logical CPUs),
Linux 6.18.60, with R 4.6.1, roxygen2 8.0.0, pkgload 1.5.3, and Rust 1.98.0.
Both workers used optimized builds in one exclusive window, with the candidate
followed by the baseline. This host differs from the initial results above;
compare workers within this window.

All 90 samples completed, with identical initial input hashes. The API-only
qualpalr copy used the title correction and rebuilt cpp11 extension described
above. Both workers received those preparations, and the producer checkout
remained unchanged. The retained
[measurements](https://github.com/jolars/diplodocus/blob/main/docs/development/benchmarks/roxygen-workflow.json)
include every sample, phase durations, diagnostics, tool observations, input
hashes, and executable hashes. The full event trace, optimized executables,
prepared extension, and implementation patch remain in local
`artifacts/native-adapters/`.

Native prose, formal, and export saves changed only maintained R source. Each
save regenerated Rd and changed the rendered site; export saves also changed
NAMESPACE. Every measured native command generated exactly once and settled
without a watch loop. An unmeasured generation error retained the snapshot and
site byte for byte. Correcting the source restored the site and returned the
server to its idle state.

Times below are **median / p95 in seconds**. Baseline API prose edits modify Rd,
and baseline formal edits modify R and Rd together. Native API edits modify R
source only. The baseline `roxygen-gap` row leaves documentation unchanged.

| Action | Baseline | Native roxygen2 |
| --- | --- | --- |
| startup | 6.249 / 6.269 | 7.919 / 8.256 |
| authored prose | 6.439 / 7.084 | 7.965 / 8.189 |
| API prose | 6.416 / 6.764 | 8.112 / 10.214 |
| formals | 6.449 / 6.637 | 8.157 / 13.685 |
| exports | — | 8.123 / 8.393 |
| roxygen-gap | 6.378 / 6.555 | — |

The baseline starts with maintained Rd ready to consume; its timings omit any
separate documentation-generation command. The native workflow worker in this
comparison ran roxygen2 generation and checkout reflection on every command
attempt, including authored saves. These measurements predate package
invalidation and extraction caching.

On native API prose saves, generation took a median 0.563 s, native package
loading across preparation and examples totaled 1.424 s, rendering took 0.152 s,
and reference resolution took 0.047 s. Native processes totaled a median
7.121 s. Helper durations overlap their host process durations. The longest
formal save took 13.685 s and contained one generation with longer native
process durations across several stages. The report retains that observation.

Use **8.3 s startup** and **13.7 s source-save p95** as initial completed-preview
comparison points on this host. Native source-save medians were about **8.1 s**.
These measurements guide the next invalidation work; they are not CI gates or
portable latency guarantees.

## Incremental workflow results {#incremental-workflow-results}

Recorded on November 17, 2026, on the Intel Core Ultra 7 155U host described
above. Both optimized workers ran in one exclusive measurement session,
baseline first. The baseline includes native roxygen2 generation and reruns the
pipeline on each save. The candidate retains verified package results between
saves and commands. Both workers received identical prepared inputs.

All 96 main samples passed: three fresh workspaces per worker and profile, with
two saves of each change type in every workspace. The retained
[measurements](https://github.com/jolars/diplodocus/blob/main/docs/development/benchmarks/incremental.json)
include exact samples, phase and cache observations, tool versions, input hashes,
and source and executable receipts. Full event traces, implementation patches,
and binaries remain in local `artifacts/native-adapters/`.

Times below are **median / p95 in seconds**. The R API prose, formal, and export
cases change maintained R source only. The monorepo signature case changes a
Python annotation. Each pair saves a new variant and then restores the original
bytes, so the medians include both invalidation and restoration.

| Profile | Action | Baseline | Incremental |
| --- | --- | --- | --- |
| qualpalr | startup | 7.662 / 7.749 | 14.672 / 14.751 |
| qualpalr | authored prose | 7.828 / 7.890 | 4.514 / 4.563 |
| qualpalr | API prose | 7.754 / 8.033 | 10.157 / 14.502 |
| qualpalr | formals | 7.855 / 7.925 | 10.150 / 14.443 |
| qualpalr | exports | 7.866 / 7.918 | 10.180 / 14.492 |
| monorepo | startup | 13.289 / 13.299 | 33.012 / 33.286 |
| monorepo | authored prose | 13.088 / 13.162 | 10.871 / 10.901 |
| monorepo | R API prose | 13.134 / 13.187 | 14.965 / 19.062 |
| monorepo | Python signature | 13.101 / 13.130 | 12.437 / 14.135 |

Authored-save medians improved by **42.3% for qualpalr** and **16.9% for the
monorepo**. Every authored save hit all expected native result caches, including
generation, extraction, metadata, and examples. Fresh environment and checkout
probes and input verification still run. Every recorded candidate save produced
exactly one rebuild. R generation failures retained the completed site and
snapshot; correcting the source recovered without a generated-write loop.

A separate acceptance run used the same candidate and inputs. After an authored
save, restarting with retained caches hit all expected native stages and
preserved every site byte and the snapshot fingerprint. Those single restart
observations took 4.731 s for qualpalr and 11.418 s for the monorepo; they are
separate from the paired startup and save samples above.

Cold starts and new R API edits remain more expensive. A new R source variant
invalidates the whole package, then pays for native work and dependency capture.
Qualpalr's new API variants took about 14.4 s, while restored variants took about
5.9 s. R API prose medians increased by 31.0% for qualpalr and 13.9% for the
monorepo. These results support reuse for authored saves and expose the remaining
cost of conservative package invalidation. Finer invalidation, persistent
workers, and provisional previews remain future work.

Input guards retain fresh verification boundaries. The implementation reduces
duplicate observations, uses hash maps for private lookup state, and checks
subscription ancestors instead of comparing every dependency root with every
other root. These changes improve bookkeeping costs without changing snapshot
or publication validation. As with the earlier measurements, this local sample
does not establish portable latency or precise tail guarantees.

## Python native migration results {#python-native-results}

Recorded on November 17, 2026, on an AMD Ryzen 9 7900 host with 24 logical CPUs.
The Python migration profile compares the previous source adapter with native
Sphinx discovery on the same prepared Python package. Both release workers used
Python 3.14.7; the native worker used Sphinx 9.1.0 and Docutils 0.23. The
[measurements](https://github.com/jolars/diplodocus/blob/main/docs/development/benchmarks/python-native.json)
retain input and executable hashes, source receipts, individual timings,
cache counts, and aggregate phase timings.

All 48 samples passed: three fresh workspaces per worker, two saves of each
change type per workspace, and three warm restarts. The workers received
identical initial files. Each restart preserved every site byte and the
completed snapshot fingerprint. Native API semantics and documentation differ
from the previous source interpretation, so outputs are compared within each
worker rather than between workers.

The comparison ran baseline then candidate in one foreground command, with no
other builds or tests active. The command ran without an exclusive reservation.
This local sample provides directional costs rather than portable latency or
precise tail guarantees.

Times below are **median / p95 in seconds**. Saves include polling and debounce.
Each pair writes a new variant and then restores the original bytes.

| Action | Source adapter | Native Python |
| --- | --- | --- |
| startup | 8.550 / 9.816 | 26.924 / 27.758 |
| authored prose | 6.668 / 7.465 | 15.704 / 16.206 |
| API prose | 7.765 / 8.874 | 22.399 / 31.023 |
| signature | 7.877 / 9.399 | 21.987 / 29.174 |
| warm restart | 6.536 / 6.653 | 14.841 / 15.406 |

Native discovery and all six doctest documents reused completed results on
every authored save and warm restart. New API variants invalidated the package;
restored variants reused their earlier completed results. New API prose variants
took a median 30.522 s, while restored variants took 16.437 s. The combined
medians in the table include both cases.

The native path currently costs more even when results are reusable. Source
verification on authored saves increased from a median 3.012 s to 5.420 s, and
watch capture increased from 0.619 s to 1.375 s. Rendering remained about
0.026 s. Conservative environment observations and publication guards remain
active on cache hits. These measurements identify verification and environment
bookkeeping as follow-up performance work. Further improvements should retain
the current publication checks.
