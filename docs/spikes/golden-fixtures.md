# Milestone 2 golden fixtures

The exploratory adapters now preserve their observed output before production
extractors and the execution engine replace them. The two decisions that use
this evidence are [static extraction](../decisions/0001-static-extraction.md)
and [authored content](../decisions/0002-authored-content.md).

These fixtures record what the selected parsers and kernels expose. They are not
the future API/execution IR schema, execution-cache artifacts, or proof that all
policy decisions are already enforced. The authored snapshots do serialize the
document adapter's existing IR.

## Inventory and producing tests

All new fixtures live under
[tests/snapshots/spikes](../../tests/snapshots/spikes). Snapshot paths below are
relative to `tests/snapshots`.

  | Golden                                                                             | Producer                                                                                                 | Preserved evidence                                                                                                                                                                                                                                                                                    |
  | ---------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
  | `spikes/python.json`                                                               | `python_exploratory_output_matches_golden` in [the Python spike](../../tests/python_extraction_spike.rs) | Static PEP 621 metadata, all six source/stub modules (using the isolated `python-dynamic-export` case for the experimental module), exports and computed export expressions, re-export targets, dataclass/property/overload decorators, parameters/annotations, docstring text and sections, source ranges, typed-marker presence, and selected stub/implementation reconciliation. |
  | `spikes/r.json`                                                                    | `r_exploratory_output_matches_golden` in [the R spike](../../tests/r_extraction_spike.rs)                | DESCRIPTION values and dependency constraints, all namespace directives, every maintained function/formal/default in the three R sources, and all four Rd topics (using the isolated `unsupported-rd` case for the experimental topic) with aliases, structured markup, positional groups, options, and unresolved dynamic expressions.                                     |
  | `spikes/failures/python-{metadata,syntax,docstring}.json`                          | The existing focused Python failure probes                                                               | Malformed metadata with its native range/message, dynamic version declarations, malformed versus version-unsupported syntax, missing docstring types, and raw-to-decoded source mismatch.                                                                                                             |
  | `spikes/failures/r-{metadata,syntax,namespace-unknown,namespace-conditional}.json` | The existing focused R failure probes                                                                    | Lossless parser recovery, malformed dependency constraints, unknown directives, and the conditional context lost by the flattened namespace iterator.                                                                                                                                                 |
  | `spikes/failures/rd-{unknown,shape}.json`                                          | The existing focused Rd failure probes                                                                   | Unknown markup with a ranged diagnostic and retained payload, plus duplicate-section shape errors whose location is structural rather than a byte range.                                                                                                                                              |
  | `spikes/authored/<acceptance-path>.json`, `spikes/authored/cases/<case>/<path>.json`, and `documents/qmd.json`       | `acceptance_authored_output_matches_goldens` in [document tests](../../tests/documents.rs)               | Complete document parse output and diagnostics for every declared baseline page and every authored overlay. `python/docs/guide.qmd` uses `documents/qmd.json`; the unsupported variant retains the former golden under its case ID.                                                                               |
  | `spikes/execution/mime-bundle.json`                                                | The protocol representation probe in [the Jupyter spike](../../tests/jupyter_execution_spike.rs)         | Supported and unknown MIME alternatives, an update to the same display, and interrupt/shutdown message types before output validation.                                                                                                                                                                |
  | `spikes/execution/canned-{python,r}.json`                                          | The corpus transport probe in the Jupyter spike                                                          | Exact submitted cells/options, shell status, ordered streams, result bundles, display/update correlation, and errors over local ZMQ connections to the deterministic test kernel.                                                                                                                     |
  | `spikes/execution/real-{python3,ir}.json`                                          | [The real-kernel probe](../../tests/jupyter_real_kernels.rs)                                             | Actual kernel-info identity/versions and five stateful cells' shell status, streams, rich representations, and error records, captured only after shutdown and successful process exit.                                                                                                               |

The authored acceptance inventory contains ten baseline pages and five authored
case overlays. Discovery follows declared collections in sorted order; corpus
metadata is not parsed as site content. Diagnostic-free GFM and QMD baseline
snapshots replace the former combined pages. The original unsupported pages and
all three execution-policy documents retain byte-identical snapshots under
`spikes/authored/cases/`. The Python and R exploratory golden contents are
unchanged because their producing tests explicitly select the corresponding
isolated dynamic cases.

The project's real documentation adds five authored snapshots under
`dogfood/docs/`, produced by `real_documentation_matches_authored_goldens` in
[the corpus gate tests](../../tests/acceptance_gate.rs). Its four-cell Python
example also has a real-kernel output snapshot at `dogfood/execution.json`,
produced by `declared_python_kernel_executes_the_real_documentation_example`.
The same declared environment and observation normalization apply to both
execution corpora.

The [case registry](../../tests/fixtures/acceptance/CASES.json) records the
complete expected check/build diagnostics, while the [acceptance
matrix](../../tests/fixtures/acceptance/MATRIX.md) maps all eleven MVP criteria
to concrete inputs, actions, outcomes, and verification milestones. Fixture
checks establish composition and parser behavior now. Semantic API diagnostics,
execution authorization, and output sanitization remain production
implementation gates; a registry entry is not proof of those behaviors.

## Representation and normalization

Static observations contain selected named fields and ordered arrays, not large
native AST debug dumps. R documentation retains its complete markup tree,
including code/verbatim leaves, groups, and options; plain text is not its sole
representation. Native enum names and diagnostic messages in the exploratory
records describe the pinned parser behavior, not Diplodocus public enums or
diagnostic codes. Successful Rd nodes deliberately have no invented byte range.

Execution observations are a projection of messages collected by the existing
probes, implemented in
[execution_observation.rs](../../tests/support/execution_observation.rs). They
exclude headers, ports, connection files, process IDs, and execution counts by
selecting fields before serialization. Display IDs become zero-based first-seen
references shared by a page's observations, preserving the relation between a
display and an update without storing the wire ID.

Only traceback presentation is normalized: strip ANSI CSI sequences and replace
IPython's `Cell In[N]` execution-count label with the producing authored cell
ordinal. Preserve the rest of each traceback, including source lines and error
text. There are no wildcard redactions, generic path scrubbing, newline
trimming, or MIME preference filtering. Unexpected machine paths must be
investigated and given a focused normalization rule before a golden is accepted.
Streams, Markdown, SVG, and plain-text alternatives retain their exact bytes and
event order. JSON object keys serialize deterministically; array order is
significant.

Real kernel-info versions are intentionally retained and tied to the declared
devenv. Python reports the IPython implementation version, which is distinct
from the installed ipykernel distribution version. The real snapshots also
retain R's extra stderr newline, its `ERROR` name, and its display events rather
than making them match the canned Python-like replies. Dependency or kernel
upgrades may require a reviewed baseline change.

The canned kernel reports an OK shell reply even for a canned IOPub error, as
the spike already asserts. The real kernels report errors in both channels.
Keeping that distinction prevents the transport fixture from masquerading as the
production allowed-error policy. Likewise, an observed update is not proof that
the future page executor has replaced all affected output slots.

## Verification and updates

Normal tests compare against checked-in bytes through the existing Snapbox
helper. Missing files or changed fields fail. The capture tests were first run
without the new goldens to verify those failures, then their generated output
was reviewed and checked in as reference data. Nothing imports the documented
Python package or sources the R package to produce static observations.

Run the same suite as CI in the declared environment:

```sh
devenv shell -- cargo test --locked --all-targets
```

For an intentional parser/corpus/kernel change, update only the affected test
targets, review every changed golden, then rerun without overwrite mode:

```sh
devenv shell -- env SNAPSHOTS=overwrite cargo test --locked \
  --test python_extraction_spike --test r_extraction_spike \
  --test documents --test jupyter_execution_spike --test jupyter_real_kernels
git diff -- tests/snapshots
devenv shell -- cargo test --locked --all-targets
```

The kernel packages must already be realized by devenv; no test installs them,
registers a spec, or starts a Jupyter server. Ordinary CI never sets overwrite
mode. Source ranges, exact outputs, and runtime-version changes are reviewable
changes, not reasons to widen snapshot matching.

During production replacement, retain these observations and add assertions for
the corresponding production facts, source attribution, diagnostics, and safe
outputs. Document intentional differences such as applying MIME selection or
display replacement. Remove a spike only after its acceptance and failure
evidence is covered through the new adapter. The contract's remaining safety,
schema, and lifecycle tests are still required.
