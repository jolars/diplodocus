# Authored-execution contract

## Outcome

The MVP executes explicitly authorized QMD pages through the `jupyter` engine.
One page owns one kernel session. Parsing, option validation, output conversion,
and rendering remain Diplodocus operations, with no Quarto, Pandoc, or Jupyter
server in the pipeline.

This is the logical contract for the Milestone 2 policy decision. It extends the
[Jupyter spike](jupyter-execution.md) and the [acceptance
matrix](../../tests/fixtures/acceptance/MATRIX.md). Milestone 3 will define
serialized execution types, and Milestone 6 will implement this policy. The
existing parser retains declarations and ranges but does not yet enforce this
entire contract. The [page execution-cache contract](page-execution-cache.md)
specifies key encoding and artifact layout; this document defines the
information that those artifacts must preserve.

Policy identifiers are `qmd-mvp-v1`, `mime-mvp-v1`, `html-mvp-v1`, `svg-mvp-v1`,
and `execution-mvp-v1`. Changing a default, supported value, selection order, or
safety rule changes the corresponding identifier. Parser and sanitizer
dependency versions are recorded separately.

## Authority and page selection

Only `build` and `serve`, with collection `format = "qmd"` and an explicit
`[content.execution]` containing `mode = "execute"`, `engine = "jupyter"`, and a
nonempty kernel name, may discover or launch a kernel. Omitted execution
configuration means `mode = "never"`. `execute` on a GFM collection is an error.
Engine and kernel settings with `never` are rejected as contradictory
configuration. API examples and every GFM fence remain display-only.

Every `check` path, and every collection with mode `never`, parses and validates
without kernel discovery, runtime probes, execution-cache reads or writes, or
execution-asset writes. `check` also avoids output-directory creation; `build`
may render unexecuted documents normally. A check can validate the declared
kernel name's syntax but cannot prove that its runtime is installed. Authored
metadata can restrict an authorized page, but cannot grant authority.

In an authorized page, braced fences whose language matches the selected
kernel's language are executable candidates. Compare ASCII case-insensitively,
normalizing `python` and `python3` to `python`, and `r` to `r`. Other language
fences and fences without braces remain display-only, with no second kernel or
language-based fallback. Kernel names such as `python3` and `ir` are selectors,
not language names. Read the configured spec only after the authority gate, and
require its language to agree with `kernel_info_reply` before sending code.

Walk authored cells in source order, including cells nested in supported
containers. A page with `execute: false`, no braced cells, or all cells having
`eval: false` needs no kernel discovery. After selecting a spec, a page with no
matching candidates needs no launch. Skipped cells produce no outputs. In a
`never` collection, retain and display cell source without applying execution
presentation options; this preserves the guide fixture's displayed input even
though it declares `echo: false`.

## QMD metadata and options

### Document metadata

Frontmatter is absent or a YAML mapping with unique scalar keys. The complete
supported top-level subset is:

  | Key        | Accepted value and default                                                                | Meaning                                                                                                                                                                    |
  | ---------- | ----------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
  | `title`    | Nonempty string; when absent, use the first authored heading, then the page filename stem | Page title, rendered and indexed as text.                                                                                                                                  |
  | `audience` | String or sequence of strings; default `[]`                                               | Normalize a scalar to a one-element list; retain order and expose the values as searchable page metadata. No execution meaning.                                            |
  | `execute`  | Boolean or mapping; absent means an empty mapping                                         | `false` disables every cell on this page. `true` supplies no defaults and is accepted only in an authorized collection. A mapping supplies the five defaults listed below. |

An `execute` mapping accepts only `eval`, `echo`, `output`, `include`, and
`error`, with the cell types and defaults below. `execute.eval: false` is an
inherited default that an individual cell may override inside an authorized
collection. Top-level `execute: false` is a page-wide veto that no cell may
override. Neither an inherited nor a cell-level `eval: true` overrides
collection mode `never`.

`jupyter`, `engine`, `kernel`, `execution`, and `execute.mode`,
`execute.engine`, or `execute.kernel` are not supported document settings. When
a document declares top-level `execute: true` or selects an engine/kernel in a
`never` collection, emit one error `document-execution-not-authorized`, with the
offending declarations as related ranges. In particular, `execute: true`
together with `jupyter: python3` in the safety fixture produces that single
diagnostic. Suppress redundant unsupported-key/type diagnostics for those
authority declarations. The same unsupported selectors in an authorized
collection produce `unsupported-qmd-metadata` errors.

Other unlisted metadata, including `format`, `params`, `filters`, `resources`,
`bibliography`, and Quarto project inheritance, produces
`unsupported-qmd-metadata` errors. Retain its source for diagnostics without
interpreting it or passing it to a kernel. Quarto's broader metadata surface is
not an implicit extension mechanism.

### Cell subset

The names follow the [Quarto Jupyter cell
reference](https://quarto.org/docs/reference/cells/cells-jupyter.html). The
restrictions and defaults here are Diplodocus policy.

  | Key          | Accepted value               | Default | Effect                                                                                                                                             |
  | ------------ | ---------------------------- | ------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
  | `eval`       | Boolean                      | `true`  | Controls whether an eligible cell is submitted.                                                                                                    |
  | `echo`       | Boolean                      | `true`  | Shows or hides input for cells in an authorized collection, including cells skipped with `eval: false`.                                            |
  | `output`     | Boolean or the string `asis` | `true`  | Shows results, hides results, or parses stdout as a non-executable Markdown fragment. Rich results keep their MIME semantics.                      |
  | `include`    | Boolean                      | `true`  | `false` hides both input and results, regardless of `echo` and `output`, while preserving evaluation and failure handling.                         |
  | `error`      | Boolean                      | `false` | Allows a language exception to become typed output and lets later cells run. It never permits transport, timeout, validation, or cleanup failures. |
  | `label`      | Nonempty string              | Absent  | Optional page-local cell anchor; never an API or concept identity.                                                                                 |
  | `fig-alt`    | String                       | Absent  | Alt text for each selected figure from the cell.                                                                                                   |
  | `fig-cap`    | String                       | Absent  | Plain-text caption for the cell's figure group.                                                                                                    |
  | `fig-subcap` | Sequence of strings          | `[]`    | Plain-text captions for individual figures in final output order.                                                                                  |

Use the fence identifier (`#setup`) as the label when no explicit `label`
exists. If both exist, they must agree. Labels must match
`[A-Za-z][A-Za-z0-9_.:-]*` and be unique among authored anchors on the page.
They do not enable Quarto's automatic figure numbering or cross-reference
machinery. A nonempty `fig-subcap` list must match the number of selected figure
assets after display updates and clearing. Diagnose a mismatch as
`invalid-figure-options` after the page finishes execution, including when an
executed cell produces no figures. Skipped cells retain the list without
inventing figures or an error.

Figures use their final selected asset representation, not every image MIME
alternative, when counting subcaptions. Figures fall back to the image's own alt
text for generated Markdown images when `fig-alt` is absent, and otherwise use
an empty alt string. Captions and alt text are escaped text, not new Markdown
input.

Unlisted cell options are errors `unsupported-cell-option`. This includes
`warning`, `message`, `results`, per-cell `cache` or `freeze`, `timeout`,
`fig-width`, `fig-height`, layout settings, `tags`, and `classes`. Arbitrary
fence classes are also diagnosed. There is no selective expression evaluation,
line selection for `echo`, implicit knitr alias, or runtime plotting-device
configuration. In particular, stderr remains a stream: Jupyter does not provide
a portable language-warning event that could justify a `warning: false` rule.

### Validation and normalization

Keep Panache's original declarations, overridden values, source segments, and
zero-based half-open UTF-8 byte ranges. Derive a separate effective option map
using this precedence, highest first:

1. Hashpipe YAML declarations in the cell preamble.
2. Inline fence options.
3. Document `execute` mapping defaults for the five inheritable keys.
4. The defaults in the table.

Use Panache's canonical lowercase, hyphenated cell keys; document metadata keys
use their exact documented spelling. Validate every declaration, even an
overridden one, without interpreting expressions. Hashpipe YAML booleans must be
unquoted `true` or `false`; inline boolean values accept those same literal
spellings. Quoted boolean strings, numbers, nulls, missing values, `yes`/`no`,
and R expressions such as `FALSE` are invalid. String options accept literal
scalars, including quoted scalars, without interpolation. Structured lists use
hashpipe YAML. YAML tags, aliases, and merge keys are unsupported.

Duplicate keys within one tier are errors, including duplicate frontmatter keys.
The parser's `ambiguous-cell-option` warning becomes an error during policy
validation without a duplicate warning. Never select an arbitrary winner. Bad
value types produce `invalid-cell-option` or `invalid-qmd-metadata`; malformed
YAML retains `invalid-embedded-yaml`. Validation runs even for skipped cells and
`check`, so disabling execution does not hide configuration mistakes.
Materialize defaults in effective options, retain ordered lists, and record each
winning value's origin as default, document, inline, or hashpipe with its source
range where present.

## Outputs and MIME preference

Preserve stdout, stderr, errors, and display/result events in IOPub order.
Ordinary streams are escaped preformatted text. For `output: asis`, concatenate
only adjacent stdout events from the same cell before parsing each resulting run
as Markdown; never reorder across stderr or display events. Stderr and
tracebacks always remain text. `output: false` and `include: false` suppress
presentation, not collection, safety validation, or the error policy.

For every display/result bundle, validate supported candidates before storing
portable IR. Retain safe alternatives in this fixed preference order; the
renderer selects the first surviving representation, independently of bundle key
order:

  | Rank | MIME type       | Portable representation                                       |
  | ---- | --------------- | ------------------------------------------------------------- |
  | 1    | `image/svg+xml` | Validated inert SVG stored as a local asset.                  |
  | 2    | `image/png`     | Strictly decoded PNG stored as a local asset.                 |
  | 3    | `image/jpeg`    | Strictly decoded JPEG stored as a local asset.                |
  | 4    | `text/markdown` | Isolated parsed Markdown blocks.                              |
  | 5    | `text/html`     | Sanitized HTML fragment, admitted only by the boundary below. |
  | 6    | `text/plain`    | Escaped preformatted text.                                    |

This order preserves a figure over a kernel's object-description fallback and
prefers structured Markdown to HTML. Decode PNG and JPEG payloads from base64,
verify their actual format, and hash the accepted asset bytes. SVG payloads are
XML text, not base64. Do not trust a MIME label or a kernel-supplied filename.
JSON, LaTeX, PDF, JavaScript, widgets, vendor media, audio, video, and
additional raster types are unsupported in v1. An unsupported alternative may be
ignored when a supported one survives; otherwise emit `unsupported-cell-output`
and a visible placeholder listing the MIME names, never the rejected active
payload. Invalid supported payloads produce `invalid-cell-output` warnings and
fall through to safe alternatives.

Markdown fragments use the in-process Panache GFM reader with execution and
semantic-target creation disabled. Braced fences become display code. No
frontmatter settings, includes, generated labels, headings, or directives can
alter page execution, navigation, or semantic identity. Safe links use the
owning page's reference context; source attribution points to the producing cell
plus a fragment-relative range, not a temporary authored file. Raw HTML inside
Markdown stays unsupported and escaped; it does not enter the kernel HTML
sanitizer as a shortcut. Never reparse the whole authored document or write
temporary Markdown.

### HTML and asset boundary

Raw authored HTML remains an unsupported source node. Kernel `text/html` is
untrusted even after an authorized execution or cache hit. Only an in-process
HTML parser and allowlist sanitizer may construct `sanitized_html` IR. Neither
the renderer nor a cache deserializer may construct that trusted variant from an
unchecked string. Record the sanitizer policy and implementation versions, and
revalidate cached markup against the active policy before rendering.

The v1 HTML allowlist is `p`, `br`, `hr`, `div`, `span`, `strong`, `em`, `b`,
`i`, `s`, `sub`, `sup`, `code`, `pre`, `blockquote`, `ul`, `ol`, `li`, `dl`,
`dt`, `dd`, `table`, `caption`, `thead`, `tbody`, `tfoot`, `tr`, `th`, `td`,
`a`, and `img`. Retain only `title`; `href` on links; `src`, `alt`, and positive
integer `width`/`height` on images; positive integer `colspan`/`rowspan` and
valid `scope` on table cells; and integer `start` on ordered lists. Rebuild a
fragment using escaped attribute values and text. Remove comments and `class`,
`id`, and `data-*` attributes, so output cannot inject site classes or targets.

Reject the HTML candidate if it contains any other element or attribute,
including scripts, event handlers, inline CSS, `style`, `link`, `base`, `meta`,
forms, frames, objects, custom elements, SVG, MathML, or `srcdoc`. This
conservative rule avoids claiming fidelity after removing active behavior,
styling, or unsupported structure. Emit one `unsafe-kernel-html` warning per
rejected HTML candidate, even if another MIME representation succeeds. Choose
the next safe alternative; if none exists, show an unsupported-output
placeholder without a second generic warning. Script-only HTML therefore never
becomes a silently empty successful result. There is no trusted-HTML option,
notebook trust bypass, or browser-side dependency loading.

Links may use `https`, `http`, `mailto`, validated page-relative destinations,
or fragments targeting existing authored anchors. Decode entities and URL
escapes before checking the scheme and path. Reject protocol-relative URLs,
`javascript:`, `file:`, `data:`, and `blob:`. Images must resolve to local
validated assets; remote images are rejected, not fetched or linked for the
browser to fetch. Apply the same URL rules to generated Markdown.

The kernel starts in the authored page's parent directory. Generated local asset
references resolve against that directory and must stay inside the content
collection's declared repository, including after symlink resolution. This
repository is the input boundary; generated files are copied into a separate
page-scoped execution-asset staging directory for publication. Reject absolute
paths, traversal outside the boundary, symlink escapes, and nonregular files
before reading bytes. A violation is an error
`generated-asset-outside-boundary`, even if another MIME alternative exists.
Missing files are `generated-asset-missing` errors. Portable asset references
contain a content digest, media type, and size, not source or staging paths.
Deduplicate identical bytes; an existing digest with different bytes is an
`execution-asset-collision` error. Only validated assets are published beneath
the owning page's execution-asset namespace.

SVG has its own XML validation boundary, including for image references inside
HTML or Markdown. Allow static geometry and text (`svg`, `g`, `path`, `rect`,
`circle`, `ellipse`, `line`, `polyline`, `polygon`, `text`, `tspan`, `title`,
and `desc`). The attribute allowlist is `x`, `y`, `x1`, `y1`, `x2`, `y2`, `dx`,
`dy`, `width`, `height`, `cx`, `cy`, `r`, `rx`, `ry`, `d`, `points`,
`transform`, `viewBox`, `fill`, `stroke`, `stroke-width`, `fill-opacity`,
`stroke-opacity`, `opacity`, `fill-rule`, `stroke-linecap`, `stroke-linejoin`,
`stroke-miterlimit`, `font-family`, `font-size`, `font-style`, `font-weight`,
and `text-anchor`. Also allow the root SVG namespace declaration and
`role="img"`. Values must parse as finite numbers, number lists, paths,
transforms, literal colors, font names, or the attribute's standard keyword
enum; paint URLs are forbidden. Reject DTDs, entities, processing instructions,
external references, `href`, CSS, scripts, handlers, animation, `foreignObject`,
and all other elements/attributes. Rejection emits `unsafe-kernel-svg` and uses
a safe alternative or placeholder. Publish accepted SVG as an image asset, never
inline HTML. The stateful fixtures' rectangle and text figures fit this subset.

This boundary controls document output and asset copying. Authored code itself
runs with the user's privileges and can write files or use the network.
Diplodocus does not sandbox or roll back those side effects.

## Execution and failure policy

The [Jupyter messaging
specification](https://jupyter-client.readthedocs.io/en/stable/messaging.html)
defines the wire events. Diplodocus owns the following page policy:

- Submit one cell at a time with `silent = false`, `store_history = true`,
  `allow_stdin = false`, empty `user_expressions`, and `stop_on_error = true`.
  Match shell and IOPub events by parent request ID. Completion requires both
  the matching shell reply and matching IOPub `idle`, in either arrival order.
- A language error in either channel fails the cell by default. Coalesce the
  shell and IOPub reports of the same exception into one typed error at the
  IOPub position, or at cell completion if only the shell reports it. With
  `error: true`, retain it without a build diagnostic and submit the next cell
  in the same session. A shell `aborted` reply always fails the page.
- Replace every surviving output slot registered for a `display_id` when a
  matching update arrives during an active cell, including slots in earlier
  cells. Keep the original slot order and record the updating cell separately.
  Unknown display IDs produce `unsupported-cell-output` warnings. Apply
  `clear_output(wait = false)` immediately to the current cell; with `true`,
  defer clearing until its next output, retaining existing output if none
  arrives. Clearing removes those slots from the display map.
- Unrelated parent IDs are never attached to a cell. Late output from a
  completed request, background output without a parent, and unsupported
  interactive messages are ignored with a visible `unsupported-kernel-message`
  warning when observed. There is no background-output drain period. Stdin
  requests fail with `execution-input-requested`; never prompt or send input.
- Missing or malformed configured kernels, language mismatches, launch errors,
  invalid/authentication-failed protocol messages, disconnected channels, kernel
  death, timeout, cancellation, and output-validation errors fail the page
  regardless of `error`, `output`, or `include`. Stop submitting cells; do not
  automatically restart or retry authored code.

All waits use monotonic deadlines. The initial fixed limits are:

  | Phase                    | Limit      | Boundary                                                                                                                                                           |
  | ------------------------ | ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
  | Startup                  | 30 seconds | Spawn through channel readiness and validated kernel info.                                                                                                         |
  | Cell                     | 60 seconds | Request submission through both terminal messages; stream activity does not extend it.                                                                             |
  | Terminal synchronization | 5 seconds  | After the first terminal message, await its counterpart within this limit and the remaining cell deadline. An otherwise quiet running cell uses the cell deadline. |
  | Interrupt                | 5 seconds  | Deliver the interrupt and await idle or process exit.                                                                                                              |
  | Shutdown                 | 5 seconds  | Graceful shutdown request, reply, and child exit.                                                                                                                  |
  | Termination              | 5 seconds  | After graceful shutdown fails, terminate the process group, then kill it if needed.                                                                                |
  | Forced exit              | 5 seconds  | Await exit and reap after kill; expiry is a cleanup error, never a successful shutdown.                                                                            |

These are engine policy, not QMD options; tests may inject shorter deadlines.
Kernel process death is monitored throughout, so no heartbeat response can
extend a stalled cell's deadline. Every channel send/read is bounded by its
current phase. Timeout diagnostics identify the phase and producing source, not
elapsed wall-clock measurements.

On timeout or cancellation, interrupt using the kernelspec's declared mode:
control-channel `interrupt_request` for `message`, or SIGINT to the owned
process group for `signal` (the default). Then run the same bounded shutdown
path used after success or any other failure: request shutdown, await exit,
escalate through termination and kill as necessary, and reap the child. Always
close channels and remove the connection file and uncommitted asset staging.
Failure to clean up is itself an error and prevents publication; never claim
success while a kernel remains owned and live.

A failed page cannot commit a cache result, assets, or a partially updated site.
`build` exits unsuccessfully; `serve` reports the failed rebuild and continues
serving the last successful site. Warnings preserve a visible safe fallback and
do not fail the default build. Allowed language errors are valid page results.
There is no fallback to stale execution after a failed fresh run.

## Toolchain requirements

Use the exact `panache-parser = 0.29.0`, `jupyter-zmq-client = 1.0.1`, and
`jupyter-protocol = 2.0.2` pins in [Cargo.toml](../../Cargo.toml), with the
Tokio version resolved in [Cargo.lock](../../Cargo.lock). Parser crates remain
in-process. The client crates are still spike dev-dependencies until the
production engine lands. The repository's Rust build toolchain is 1.98.0; users
of a built executable do not need Rust to run it.

The supported execution baseline is the Linux environment in
[devenv.nix](../../devenv.nix) and [devenv.lock](../../devenv.lock): Python with
`ipykernel`, and R with `IRkernel` and `IRdisplay` through its dependencies. The
declared specs are `python3` and `ir`. Both are launched directly and tested in
[CI](../../.github/workflows/ci.yml) after environment realization. Users must
supply their cells' additional packages and environment inputs. Diplodocus never
installs packages, registers kernels, invokes a Jupyter server, or runs a
language command merely to discover environment paths. Other installed kernels
must satisfy this protocol contract; they are not part of the verified MVP
language set. Other operating systems need verified process-tree interruption
and cleanup before claiming execution support.

Static kernelspec search uses this documented Linux order:

1. Entries in `JUPYTER_PATH`, in order, each with `kernels/<name>/kernel.json`.
2. `JUPYTER_DATA_DIR`, or `${XDG_DATA_HOME}/jupyter`, or
   `~/.local/share/jupyter` when neither is set.
3. `/usr/local/share/jupyter`, then `/usr/share/jupyter`.

The [Jupyter data-directory
convention](https://docs.jupyter.org/en/latest/use/jupyter-directories.html)
puts `JUPYTER_PATH` ahead of other locations. Diplodocus deliberately omits
implicit `sys.prefix` discovery; expose a virtual environment's data directory
through `JUPYTER_PATH`. Normalize and deduplicate search roots while preserving
order. Search only the configured case-insensitive ASCII kernel name, containing
letters, digits, `-`, `.`, or `_`, excluding `.` and `..` and never accepting a
path. First match wins; diagnose a shadowed match with `shadowed-kernelspec`,
and fail same-directory case ambiguity. An unreadable or malformed selected spec
is an error, not permission to fall through to another spec. Record searched
locations by class and ordinal without exporting absolute paths.

Validate `argv`, a nonempty language, and an interrupt mode of `signal` or
`message`. The [kernelspec
format](https://jupyter-client.readthedocs.io/en/stable/kernels.html#kernel-specs)
supplies the launch argument vector and `{connection_file}` substitution.
Require that placeholder, invoke the vector directly without an added shell, and
resolve a bare executable through the build environment's `PATH`. The MVP
accepts literal kernelspec environment values over the inherited build
environment; `${ENV_VAR}` expansion is rejected explicitly rather than passed
through incorrectly. Local launch data may contain paths and secrets and stays
out of portable provenance.

Require a successful Jupyter protocol-major-5 kernel-info reply with nonempty
`implementation`, `implementation_version`, `language_info.name`, and
`language_info.version`. Record the exact reported protocol minor version; do
not require newer optional messages such as `iopub_welcome` from older kernels.
Startup must establish IOPub readiness within its deadline. Bind connections to
loopback and use a fresh authentication key and a private connection file for
each owned session.

## Execution provenance

Provenance uses Diplodocus-owned types. Engine version is the Diplodocus crate
version, as for the static extractors. The following fields are required logical
data, not a wire-schema declaration:

  | Scope                                     | Required fields                                                                                                                                                                                                                                                                                                                                                                                                  |
  | ----------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
  | Authored page, including unexecuted pages | Repository and collection IDs, normalized repository-relative page path, source fingerprint, source format `qmd`, Panache version, QMD policy ID, normalized collection mode, page veto, and effective cell options with declaration origins.                                                                                                                                                                    |
  | Executed or restored page                 | Engine ID `jupyter` and version; execution/MIME/HTML/SVG policy IDs; transport, protocol, fragment parser, sanitizer, and image-validator component names and exact versions; selected kernel name; kernelspec and resolved launch fingerprints; search-location classes and ordinals; interrupt mode; kernel-reported implementation, implementation version, language, language version, and protocol version. |
  | Execution environment                     | Ordered records of declared input repository ID, normalized path, and content fingerprint; page working directory as a repository-relative path; normalized deadline values; supported platform identity. Missing or escaping declared inputs are validation errors.                                                                                                                                             |
  | Page result                               | `origin = executed` or `cache`, complete ordered cell results, and stable diagnostics. Failed attempts remain diagnostics and are not successful page provenance.                                                                                                                                                                                                                                                |
  | Cell                                      | Zero-based authored cell ordinal, optional label, fence span and source segments, submitted-source fingerprint, canonical effective options, option origins, and outcome `ok`, `allowed-error`, or `skipped` with reason. Use authored ordinals, not kernel execution counts, as portable cell identity.                                                                                                         |
  | Output                                    | Owning cell ordinal, original producing cell ordinal, optional latest updating cell ordinal, stable output-slot ordinal, kind (`stream`, `display`, or `error`), stream name where applicable, offered MIME names sorted lexicographically, accepted representations in preference order, selected MIME type, and fallback/validation diagnostic references.                                                     |
  | Representation or asset                   | Media type, representation kind, accepted-content fingerprint, producing cell, sanitizer/validator policy where applicable, and asset digest and byte size for local assets. Generated fragment ranges are relative to that output and point back to its cell.                                                                                                                                                   |

All fingerprints carry their algorithm identifier. Fingerprint original source
bytes, submitted cell bytes, declared environment file contents, and accepted
output/asset bytes separately. Launch fingerprints cover the selected spec's
behavioral fields, resolved executable identity, and explicit environment
overrides without publishing argument vectors or environment values. Normalize
repository roots to repository IDs and session-specific paths to placeholders
before fingerprinting launch data. Exact canonical encoding and page-cache key
composition follow the [cache specification](page-execution-cache.md).

Declared environment files describe reproducibility inputs, not an installer or
proof of every installed package version. Do not import documented packages or
run arbitrary version probes to enrich provenance. Inherited environment values,
undeclared dependencies, randomness, time, and network access remain outside
reproducibility guarantees; never pretend a lockfile fingerprint proves those
states were enforced.

A cache hit preserves the producing engine/kernel/toolchain records and cell
results, changing only the page's result origin. Unexecuted pages have authored
provenance but no fabricated engine, runtime version, or output origin. No
portable record contains Jupyter message/session/display IDs, connection files,
ports, keys, process IDs, execution counts, timestamps, durations, temporary
paths, absolute checkout paths, or environment values. Normalize known paths in
tracebacks to repository-relative source references and replace other machine
paths with a stable external-frame marker; strip terminal control sequences.
Kernel-authored arbitrary text is not silently rewritten as deterministic
content. Its reproducibility remains the author's responsibility.

## Evidence and implementation gates

  | Contract area          | Current evidence                                                                                                                                                                                                 | Required enforcement in later milestones                                                                                                                                                       |
  | ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
  | Metadata and options   | [Document tests](../../tests/documents.rs) retain frontmatter, inline/hashpipe precedence, structured `fig-subcap`, spans, and ambiguity; guide and safety fixtures supply concrete inputs.                      | Test every accepted type, default, override, duplicate, unsupported key, page veto, skipped cell, and authority diagnostic before implementing validation.                                     |
  | MIME and fragments     | [Protocol spike](../../tests/jupyter_execution_spike.rs) retains MIME alternatives and display IDs; generated-Markdown and stdout fixtures define the fragment boundary.                                         | Prove ranking is independent of bundle order, as-is stream grouping, inert generated fences, and fallback/placeholder behavior.                                                                |
  | HTML and assets        | Unsafe-HTML and boundary-escape fixtures fix the diagnostic codes and severity; both stateful pages supply inert SVG.                                                                                            | Test allowlists, encoded unsafe URLs, SVG active content, hidden outputs, symlink escapes, missing assets, collisions, and sanitizer revalidation on cache hits.                               |
  | Failures and lifecycle | Protocol tests exercise interruption messages, read cancellation, updates, and shutdown; [real-kernel tests](../../tests/jupyter_real_kernels.rs) verify actual error replies and clean successful process exit. | Test allowed errors followed by another cell, every timeout phase, failed startup, stdin rejection, clear/update ordering, process-tree cleanup, and preservation of the last successful site. |
  | Toolchain              | The declared devenv and CI probes execute both stateful pages without a server or installation during tests.                                                                                                     | Test exact search precedence, malformed/shadowed specs, language mismatch, unsupported interrupt modes, and absence of discovery on `never` and `check` paths.                                 |
  | Provenance             | Existing fixtures require portable source attribution and observed kernel versions.                                                                                                                              | Golden-test normalized options and complete provenance, fresh/cache parity, path redaction, and exclusion of transient protocol fields.                                                        |

These are production implementation gates, not claims that the exploratory tests
already enforce the new policy. Production enforcement remains on the existing
milestone checklist.
