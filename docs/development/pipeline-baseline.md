# Pipeline baseline

The developer benchmark measures the production preview loop through
`CommandRunner::serve_with`. It records startup until serving readiness and each
source save until a successfully published rebuild. Save times include polling
and debounce. Browser refresh transport and browser rendering are not measured.

## Run a comparison

Build the worker before acquiring exclusive benchmark access:

```sh
compute run -- cargo build --locked --release --example pipeline_bench
mkdir -p artifacts/native-adapters
cp target/release/examples/pipeline_bench artifacts/native-adapters/baseline
```

Apply the candidate change, build again, and compare both executables in a single
foreground reservation:

```sh
compute run -- cargo build --locked --release --example pipeline_bench
compute bench --reason "native adapter baseline and candidate" -- \
  python scripts/bench-pipeline.py \
  --worker baseline=artifacts/native-adapters/baseline \
  --worker candidate=target/release/examples/pipeline_bench \
  --output artifacts/native-adapters/comparison.json
```

For the initial boundary refactor, the baseline is the parent implementation
with the measurement instrumentation and worker added. Future comparisons can
use the worker available in both revisions. Archive executable digests, source
revisions, and any instrumentation-only patch with the measurement artifacts.

The default cases are the four-language monorepo example, Qualpal public
headers, and qualpalr API documentation. Supply different producer checkouts
with `--qualpal` and `--qualpalr`. Both executables must inherit the same prepared
environment. For local qualpalr measurements, its existing
`.devenv/profile/bin` can supply the prepared R wrapper on PATH. The harness
does not create environments, install packages, or compile producer extensions.

Each executable receives equivalent temporary copies. The Qualpal profile uses
a headers-only Doxyfile. The qualpalr profile selects maintained API inputs,
native artifacts, runtime assets, package metadata, and news, and excludes
vignettes. The original producer checkouts are never edited. Missing tools,
dependencies, incompatible compiled extensions, or failed builds produce
explicit blocked cases rather than substitute packages or successful timings.

The harness performs one unreported warmup, five fresh-workspace startup runs,
and two saves of each mutation kind per startup. That gives ten warm-save
observations per kind. Startup has fresh Diplodocus caches but shares warmed OS
and native runtime caches. It is not a machine-cold boot measurement. Warm saves
reuse the same running server and its workspace caches.

Mutations cover authored prose, API prose, and signatures. R signature edits
update source formals and maintained Rd usage together because the current
adapter rejects disagreement. The qualpalr case
also edits maintained roxygen prose and verifies that Rd and the rendered site
remain unchanged: that is today's generation gap, not a successful roxygen
preview update. Each change is restored before the next mutation category.
`--scenario authored --samples 1 --saves 2` provides a small harness smoke test.

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
durations without starting a runtime. Absent roxygen generation has no timing.

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
workers in one exclusive `compute bench` window. The runtime versions were
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
**5.05 s**. Roxygen generation needs its own completed-preview budget once step 2
exists. These are measured comparison points, not CI gates or performance
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
`compute` excludes other participating jobs but does not control CPU frequency,
thermal state, or unrelated system activity. Keep the measured breakdown as the
baseline for native orchestration and package invalidation work.

