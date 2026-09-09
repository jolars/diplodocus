# 0002: Parse authored content into IR and execute through Jupyter

Status: Accepted. Date: 2026-09-09.

## Context

Authored documentation must participate in the same document model, semantic
references, navigation, and search as extracted API documentation. It must
retain source locations and show unsupported syntax. Executable content
additionally needs explicit authority, page-scoped state, portable outputs, and
a controlled HTML/asset boundary.

Diplodocus supports named compatibility profiles rather than promising every
Quarto or Pandoc feature. Parsing a document must not implicitly execute it.

## Decision

### Parsing and the document boundary

Use `panache-parser = 0.29.0` in-process. Each content collection explicitly
selects `gfm` for `.md` or `qmd` for `.qmd`; the adapter selects Panache's GFM
or Quarto flavor respectively. GFM fences are display-only. QMD adds the
supported braced executable fences, hashpipe options, frontmatter, and callouts.

Consume Panache's typed block/inline views, embedded-YAML diagnostics, source
ranges, and cell source/options directly into Diplodocus document IR. Retain
unsupported nodes with their source rather than flattening them. Keep original
option declarations and their origins while deriving separately validated
effective values. Panache's CST is an adapter input, not portable IR.

The implemented [document adapter](../../src/documents.rs) and its
[tests](../../tests/documents.rs) exercise this typed surface. The
[authored-execution contract](../spikes/authored-execution-contract.md) fixes
the supported metadata/options, precedence, authority diagnostics, and
presentation behavior. Its remaining policy enforcement belongs to the later
configuration and execution milestones.

### Execution authority and lifecycle

Use engine `jupyter` behind one Diplodocus-owned adapter, with
`jupyter-zmq-client = 1.0.1`, `jupyter-protocol = 2.0.2`, and the Tokio version
resolved in [Cargo.lock](../../Cargo.lock). The protocol crate supplies wire
types; the ZMQ client supplies transport and launch-command facilities.
Diplodocus owns discovery policy, the process, request matching, output
collection, deadlines, interruption, and cleanup.

Only explicitly configured `mode = "execute"` QMD collections with an engine and
kernel can discover or start a kernel for `build` or `serve`. Omission means
`never`. Document metadata may restrict existing authority but cannot grant it.
`check` and `never` paths avoid discovery, runtime probes, execution cache
access, and execution-asset writes. API examples remain display-only.

One page owns one session. Send eligible matching-language cells in authored
order and require both the matching shell reply and IOPub idle. Preserve stream
order, allowed language errors, clearing, and updates to earlier display slots.
Apply the execution contract's bounded startup, cell, interrupt, and shutdown
policy on success and failure; reap the child before publishing results.
Transport, validation, timeout, and cleanup failures cannot be permitted by a
cell's `error` option. A failed rebuild leaves `serve`'s last successful site
available.

The supported baseline provides Python/ipykernel and R/IRkernel through
[devenv](../../devenv.nix). Tests launch these declared kernels directly over
local ZeroMQ, without a Jupyter server, installation, or registration during the
test. Authored code runs with the user's privileges; output sanitization does
not sandbox its filesystem or network effects.

### Output and cache boundary

Execution attaches structured cell results to the existing document. Ordinary
stdout, stderr, and tracebacks remain text. Parse `text/markdown` and explicitly
requested `output: asis` stdout as isolated GFM fragments with execution and
semantic-target creation disabled. Generated headings, fences, and metadata
cannot become new page identities or executable cells.

The selected MIME preference is SVG, PNG, JPEG, Markdown, HTML, then plain text.
Validate supported candidates and preserve safe alternatives. Only an in-process
allowlist sanitizer may construct trusted HTML; authored raw HTML stays
unsupported. Validate SVG separately, decode raster bytes, and copy accepted
assets from inside the owning repository boundary into a page-scoped namespace.
Rejected active payloads never become trusted output or stored placeholders.
Detailed allowlists, fallback diagnostics, and fragment rules remain
authoritative in the execution contract.

Use the [page-cache contract](../spikes/page-execution-cache.md): canonical
versioned keys cover whole source, effective options, engine/build/kernel
identity, relevant toolchain/policy versions, and declared environment files.
Store the complete page's structured results and content-addressed assets.
Verify current runtime identity through kernel startup even on a hit, skip
authored cells on a successful restore, revalidate HTML/assets, and publish
atomically after cleanup. No failed or stale partial page is reusable.

## Rejected alternatives and boundaries

  | Alternative                                                                                   | Decision and reason                                                                                                                                                                                                                                                           |
  | --------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
  | Q2 as the authored parser or execution pipeline                                               | Reject for this MVP. Keep one selected Panache-to-IR surface and a separately authorized Jupyter engine; adopting another publishing pipeline would add a second compatibility and policy boundary without evidence needed by the current corpus.                             |
  | Panache CLI, Quarto CLI, or Pandoc subprocess                                                 | Reject. The source reader is an in-process Rust dependency, and Diplodocus owns the page/site pipeline.                                                                                                                                                                       |
  | Panache's Pandoc-native or Pandoc-JSON projectors                                             | Reject as interchange formats. Consume typed syntax directly so QMD declarations, original ranges, unsupported nodes, and source-sensitive diagnostics remain under the adapter's control. A projector's presence does not make Pandoc AST the document contract.             |
  | Temporary Markdown after execution or Rd conversion                                           | Reject as a handoff to the page reader. It obscures producing-cell/source attribution, forces another document parse, and risks treating generated syntax as authored content. Isolated inert Markdown fragments are the explicit exception, with no temporary authored file. |
  | Direct HTML from parsers, extractors, raw authored nodes, or Jupyter values into the renderer | Reject. They bypass the common typed IR, reference context, and safety validation. Kernel HTML is admitted only through the sanitizer boundary, including on cache restore.                                                                                                   |
  | A new Markdown grammar or a generic GFM parser with ad hoc QMD preprocessing                  | Reject. The selected Panache surface already provides the supported profiles, typed cell declarations, diagnostics, and ranges; preprocessing would introduce another source map and grammar.                                                                                 |
  | A Jupyter server, notebook command pipeline, or direct Python/R script execution              | Reject. Direct client-to-kernel messaging supplies the required streams, rich MIME bundles, errors, and display updates while Diplodocus retains lifecycle ownership.                                                                                                         |
  | Per-cell caching, raw protocol dumps, or rendered HTML as cached results                      | Reject. They lose page state, retain transient runtime identities, or bake in site state and unchecked output.                                                                                                                                                                |

Q2 here means [Quarto's Rust rewrite](https://github.com/quarto-dev/q2), whose
README describes an experimental implementation with unstable APIs as reviewed
on 2026-09-09. Its authors also describe [parser source maps and structured
diagnostics](https://opensource.posit.co/blog/2026-05-07_quarto-2-parsing/). The
rejection is our integration decision; it is not a claim that Q2 lacks source
locations or that all its crates require an external runtime. No Q2 acceptance
benchmark was performed for this decision.

## Evidence and consequences

[Document goldens](../spikes/golden-fixtures.md) preserve the complete parse
output of all 13 authored acceptance pages, including display-only/safety pages
and unsupported syntax. The [Jupyter spike](../spikes/jupyter-execution.md)
records capability selection and the policy work the client crates leave to
Diplodocus. Its tests preserve deterministic canned Python/R messages, MIME
alternatives, display-update relationships, and separately observed real
Python/R outputs in [execution goldens](../../tests/snapshots/spikes/execution).
The real snapshots retain kernel-reported versions and successful cell results,
including language errors, after a verified shutdown.

These observations do not implement MIME selection, sanitization, the page
cache, or the full failure state machine. Milestones 3 and 6 must enforce the
contracts with focused failing tests. Keep wire observations as regression
evidence while production tests assert Diplodocus-owned output types. A parser
upgrade, new MIME type, or wider QMD compatibility requires an explicit policy
and fixture update, not an implicit expansion of execution authority.
