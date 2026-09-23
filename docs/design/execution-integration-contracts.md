# Remaining execution integration contracts

This M6-01R decision completes the six boundaries in the
[remaining-work plan](execution-remaining-plan.md). It specifies interfaces to
implement in M6-04 through M6-08, not implemented validators, storage, or an
engine. The [cache contract](../spikes/page-execution-cache.md) and
[output policy](../spikes/authored-execution-contract.md) remain authoritative.
Workspace v1 keeps its existing fields and `sanitized-html` tag. The separate
cache schema keeps `execution-result-v1` and `html-candidate`.

## Ownership and consumer signatures

The signatures below are the implementation agreement. Opaque types have
private fields, immutable accessors, and no `Serialize` or `Deserialize`
implementation unless identified as untrusted data. They do not imply adding
unused production scaffolding in M6-01R. The coordinator registers modules and
integrates changes to shared records, preparation, and diagnostic emission.

| Owner | Types and operations |
| --- | --- |
| M6-04 | Restricted canonical value/codec, domain hashes, `IdentityInputs`, `InputSnapshot`, `ExecutionIdentity`, exact build observations. No safety, cache, discovery, or session dependency. |
| M6-05 | `ValidatedMarkdown`, `ValidatedHtml`, `DecodedMarkdown`, `DecodedHtml`, `AssetUse`, `VerifiedAssets`, `AuthoredOutputContext`, `OutputOrigin`, `ExecutionDiagnostic`, `RestoreRejection`, canonical fragment content, image traversal, live/restore safety. No identity, cache, or session dependency. |
| Coordinator with M6-06 | Preparation capture and source binding, wiring typed diagnostic emission, immutable result carrier, reducer adapter, resolved launch plan, engine inputs. |
| M6-07 | Exact artifact DTO codec, projection between DTOs and M6-05 decoded types, cache filesystem. No session ownership. |
| M6-08 | Supervised ready-session lookup, provisional-hit acceptance, publication. |

As an explicit M6-05 acceptance condition, the coordinator lands and validates a
small shared-record checkpoint using reviewed safety wrappers and accepted
M6-04 types before closing M6-05 and releasing M6-06 and M6-07 in parallel. That
checkpoint defines `PreparedExecution`, `ValidatedPage`, the private
`PageExecutionResult` fields/accessors, and their contract tests using the
accepted safety wrappers. It also registers their shared exports. M6-07 imports
these records, never the engine implementation. Preparation capture, reducer
wiring, session composition, and storage bodies remain their owners' subsequent
work. This keeps the existing dependency graph: both consumers depend on
accepted M6-04 and M6-05, without a hidden additional readiness condition.
M6-05's safety implementation still compiles independently of identity. The
checkpoint is not a new 05/07 dependency cycle or authorization for unused
M6-01R production scaffolding.

### Validation evidence and final assets

```rust,ignore
// M6-05; neither content type grants trust on deserialization.
pub struct AssetUse { pub digest: Fingerprint, pub media_type: String, pub byte_size: u64 }
pub struct DecodedMarkdown { pub blocks: Vec<FragmentBlock> }
pub struct DecodedHtml { pub markup: String }
pub struct VerifiedAssets { /* private map of validated, staged ExecutionAsset values */ }
pub struct ValidatedMarkdown { /* private blocks and ordered image bindings */ }
pub struct ValidatedHtml { /* private allowlisted owned tree and image bindings */ }
pub enum Validation<T> {
    Accepted { value: T, diagnostics: Vec<ExecutionDiagnostic> },
    Rejected { diagnostics: Vec<ExecutionDiagnostic> },
}
pub fn validate_markdown_live(
    fragment: MarkdownFragmentParse, origin: &OutputOrigin,
    context: &AuthoredOutputContext,
    assets: &mut PageAssetStore,
) -> Result<Validation<ValidatedMarkdown>, ExecutionFailure>;
pub fn validate_html_live(
    markup: &str, origin: &OutputOrigin, context: &AuthoredOutputContext,
    assets: &mut PageAssetStore,
) -> Result<Validation<ValidatedHtml>, ExecutionFailure>;
pub fn restore_markdown(
    decoded: DecodedMarkdown, origin: &OutputOrigin,
    context: &AuthoredOutputContext, assets: &VerifiedAssets,
) -> Result<ValidatedMarkdown, RestoreRejection>;
pub fn restore_html(
    decoded: DecodedHtml, origin: &OutputOrigin,
    context: &AuthoredOutputContext, assets: &VerifiedAssets,
) -> Result<ValidatedHtml, RestoreRejection>;
impl ValidatedMarkdown {
    pub fn canonical_content(&self) -> &DecodedMarkdown;
    pub fn referenced_assets(&self) -> impl Iterator<Item = &ExecutionAsset>;
    pub fn blocks(&self) -> &[Block];
}
impl ValidatedHtml {
    pub fn canonical_content(&self) -> &DecodedHtml;
    pub fn referenced_assets(&self) -> impl Iterator<Item = &ExecutionAsset>;
}
```

`FragmentBlock` and `FragmentInline` are the typed, storage-independent trees
defined below. M6-05 owns their definitions. They may be constructed as untrusted
inputs, but no conversion from them directly to a validated wrapper is public.
`VerifiedAssets` can be created only through active image-byte validation and
staging, including digest, media, size, and collision checks. Its entries expose
no mutable bytes. Cache supplies already read bytes through the existing
`PageAssetStore` validation operation and builds this table, never through an
unchecked map constructor. Restore has no original-image-path resolver.

Live and restored validation use the same immutable `AuthoredOutputContext`:
repository ID, normalized page path, collection ID, and the complete authored
anchor set from successful preparation. The coordinator preserves the preparer's
anchor map rather than deriving targets from generated output. A page-local
fragment must name an authored anchor. Generated headings and cell-like fences
cannot add anchors. Semantic references remain unresolved for the current site.
`OutputOrigin` contains the producing cell, producer slot, authored cell span,
and optional fragment identity `(ordinal, byte_length)`. Fragment ordinals
increase per producing cell, including repeated updates at the same producer
slot, and are independent of surviving output slots.
Allocate this identity while the original fragment bytes are available and
require the parsed fragment's provenance to agree with it. The parse record
alone does not contain a fragment ordinal or byte length. M6-05 defines these
context, origin, diagnostic, and rejection types with the coordinator; M6-06
supplies values and emission hooks. Thus no M6-05 signature depends on a type
first implemented by a downstream task. `RestoreRejection` has variants
`Structure`, `Url`, `UnboundAsset`, `AssetMismatch`, and `NonCanonical`; none
turns a rejected alternative into a successful partial restore.

`ValidatedMarkdown` privately owns both portable blocks and bindings addressed
by paths of `(edge, index)`: root `blocks`, then `inlines`, `blocks`, `items`,
`rows`, `cells`, `caption`, or `alt` as defined by the tree below. Non-array
edges need no invented index. For example,
`blocks[0].items[0].blocks[0].inlines[2]` identifies a nested image even if its
target, span, or digest equals another image's. Each image has exactly one
binding and each binding addresses an image. Portable image targets are replaced
with the corresponding `ExecutionAsset.reference.path`. Renderers obtain URLs
from bindings and the current publisher, never by parsing those target strings.

```rust,ignore
// Coordinator shared-record checkpoint, before parallel M6-06/M6-07.
pub struct ValidatedPage { /* private record and slot-owned validated values */ }
pub struct PageExecutionResult { /* private ValidatedPage and owned staging */ }
impl ValidatedPage {
    pub fn record(&self) -> &PageExecutionRecord;
    pub fn portable_record(&self) -> PageExecutionRecord; // An untrusted clone.
    pub fn representation(&self, cell: usize, slot: usize, index: usize)
        -> Option<ValidatedRepresentationRef<'_>>;
    pub fn referenced_assets(&self) -> impl Iterator<Item = &ExecutionAsset>;
}
impl PageExecutionResult {
    pub fn validated(&self) -> &ValidatedPage;
    pub fn staged_assets(&self) -> &[StagedExecutionAsset];
}
pub enum ValidatedRepresentationRef<'a> {
    Text(&'a str), Markdown(&'a ValidatedMarkdown), Html(&'a ValidatedHtml),
    Asset(&'a ExecutionAsset),
}
```

The coordinator changes `PageExecutionResult` and migrates existing consumers
at that checkpoint. Its public `record` and `staged_assets` fields cannot remain aliases into
the trusted carrier. There is no mutable accessor, `DerefMut`, public parts
constructor, or reconstruction from `PageExecutionRecord`. Cloning a portable
record never clones trust. Cache encoding and safe rendering consume the
carrier; workspace projection consumes only its untrusted portable view.
Compile-fail tests must cover construction, mutation, and Serde on the wrappers
and carrier when these types are implemented.

Reducer slots own validated values, rather than storing an external map keyed
by producer and slot. Each replacement atomically replaces portable content,
evidence, provenance, and wrappers for every registered owning slot. Clears
remove all four. Final lookup uses `(owning_cell, slot, representation_index)`
on the immutable finalized carrier. Producer slots may repeat, so they are
never authority keys. Slot gaps and original producing cells survive updates;
representation producer and latest updating cell describe the replacement.

Final assets are the deduplicated union of top-level assets and every binding in
every surviving Markdown/HTML alternative, including hidden cells and unselected
alternatives. Reject digest/media/size inconsistencies. Exclude superseded or
cleared assets, but never erase their warnings or an earlier fatal failure.
Validate figure options before filtering visibility; retain staging only after
cleanup and input revalidation. M6-06 owns this `OutputReducer::finish` change.

### Canonical representation content

M6-05 returns its own typed canonical content, not an identity-module value or
encoded JSON. M6-06 and M6-07 project that same content into M6-04's restricted
value and use the following mapping. M6-05 can compile and test independently
of both identity and cache. `C` and `H` have the existing cache definitions.

| Representation | `content` | `content_digest` |
| --- | --- | --- |
| Literal `text` | `{type:"literal", text:string}` | SHA-256 of the exact text UTF-8 bytes. |
| Error `text` | `{type:"error", name:string, value:string, traceback:[string]}` | `H("diplodocus/execution-representation-v1", {kind:"error", content})`. |
| `markdown` | `{blocks:[FragmentBlock]}` | `H("diplodocus/execution-representation-v1", {kind:"markdown", content})`. Includes typed image digest/media/size, not original targets. |
| `html-candidate` | `{markup:string}` | SHA-256 of canonical accepted markup UTF-8 bytes after the internal image rewrite. |
| `asset` | `{asset:AssetUse}` | SHA-256 of exact validated image bytes, equal to `asset.digest`. |
| `unsupported` | `{mime_types:[string]}` | `H("diplodocus/execution-representation-v1", {kind:"unsupported", content})`. Diagnostic references remain on the enclosing output, outside content. |

Selection, producer, policy, sanitizer/parser provenance, and digest itself are
outside content. Fragment node spans remain inside the fragment tree; authored
cell attribution and fragment identity are representation provenance outside
the content digest. The existing raw-Markdown hash is replaced in the M6-06
adapter; cache and live execution may not apply different formulas.

Canonical HTML uses the HTML fragment parser's owned allowlisted tree. Emit
lowercase HTML element and attribute names, attributes in ASCII name order,
double-quoted values, and no formatting whitespace of the serializer's own.
Preserve text whitespace. Escape `&`, `<`, and `>` in text, and additionally
`"` in attributes, as `&amp;`, `&lt;`, `&gt;`, and `&quot;`. Void elements
`br`, `hr`, and `img` have no closing tag or slash; other elements have explicit
end tags. Parser-inserted allowed nodes remain. Serialize validated integers in
shortest decimal spelling (HTML `ol.start` may be negative; it remains inside
a string, never a negative JSON number). Drop only the policy's comments and
discardable attributes. Encode image `src` exactly as
`diplodocus-asset:sha256:<hex>`. Restore accepts only this spelling, binds it to
verified bytes, reruns the allowlist, and requires identical canonical output.
Live validation rejects this scheme. Published HTML substitutes asset URLs
from typed bindings and never emits the internal spelling.

## Exact cache DTOs

All fields below are required; nullable fields use explicit `null`. Objects have
exactly their listed fields; unknown fields/variants and duplicate keys fail
decoding at every depth. Lists retain order except the explicitly sorted sets.
Numbers are u64. These tables define explicit projection, not permission to
serialize arbitrary workspace IR with derived Serde. DTOs are untrusted.

The manifest is exactly `{schema,key,key_input,result_digest,result}` as in the
cache contract; `key_input` retains its existing exact schema and key vector.
`result` is exactly `{ir_schema,provenance,cells,diagnostics,assets}`.

| Record | Exact fields and meanings |
| --- | --- |
| Provenance | `origin:"executed"`; `page`, `engine`, `policies`, `components`, `kernel`, `platform`, `deadlines_ms`, `environment_inputs` copied exactly from `key_input`; `preparation` as below. No private launch data. |
| Preparation | `parser_version:string`, `defaults` (the five materialized key defaults), `default_origins` (the same five keys mapped to origins), `declarations:[Declaration]`. |
| Declaration | `{cell:u64\|null, kind, key:string\|null, raw:string, span:Span}`. Kind is `document`, `inline`, `hashpipe`, or `fence-identifier`; raw is the exact page slice at span. Retain every execution-option declaration, including overridden and unsupported declarations, in source order. `cell:null` denotes a document default. Raw data stays text, not arbitrary YAML values. |
| Span | `{start:u64,end:u64}`; zero-based half-open UTF-8 byte range. Authored ranges are checked against the current source, fragment ranges against the fragment's declared byte length and tree consistency. |
| Origin | `{kind:"default"}`, or `{kind,span}` where kind is `document`, `inline`, `hashpipe`, or `fence-identifier`. |
| Cell | Exactly `ordinal,label,span,source_segments,submitted_source_digest,effective,option_origins,outcome,skip_reason,outputs`. No new `language` field; language is in the corresponding key cell. |
| Source segment | `{text:string,span:Span}`, matching the prepared source segments exactly. |
| Cell options | `effective` has the same nine keys and values as the key cell. `option_origins` has those nine keys mapped to Origin. `label` equals `effective.label`. `output` maps Show/Hide/AsIs to `true`/`false`/`"asis"`. |
| Cell outcome | `ok`, `allowed-error`, or `skipped`. Only skipped cells have a nonnull `skip_reason` (`language-mismatch` or `eval-false`, in that precedence), null submitted digest, and empty outputs. Allowed error requires `effective.error:true`. |
| Output | Exactly `kind,stream,owning_cell,producing_cell,updating_cell,slot,offered_mime_types,selected_mime_type,representations,diagnostic_indices`. Kind is `stream`, `display`, or `error`. Stream is `stdout`/`stderr` for streams, otherwise null. Updating cell is nullable. MIME names are unique ASCII strings sorted lexically; accepted alternatives use policy preference order. |
| Representation | Exactly `kind,media_type,content,content_digest,producing_cell,policy,producer,fragment`. Kinds/content are in the digest table. Policy and producer are nullable; producer is `{name,version}`. Fragment is null except Markdown, where it is `{ordinal,slot,byte_length}` from the producing context. |
| Asset use/table entry | `{digest,media_type,byte_size}`. The result asset table is unique and sorted by digest; every entry is used and every use has identical metadata. No portable publication path enters this table. |

Markdown uses `media_type:"text/markdown"`, policy `qmd-mvp-v1`, and producer
`panache-parser` with the fragment parser's exact version. HTML uses
`media_type:"text/html"`, policy `html-mvp-v1`, and the producing sanitizer's
name/version. SVG uses its media type and `svg-mvp-v1`; raster assets use their
media type and `mime-mvp-v1`, matching existing accepted evidence. Asset
producers identify the active validator. Literal
and error text use `text/plain`, null policy, and null producer. Unsupported
uses null media type, `mime-mvp-v1`, and null producer. Non-Markdown fragments
are null. Digest strings always use `sha256:<64 lowercase hex digits>`.

An error output has one error-text representation, empty offered MIME names,
null selection, and no update. The workspace projection puts name/value/traceback
in `CellOutputKind::Error` (value maps to `message`), with empty representation
and evidence vectors as the existing reducer does. A display with no accepted
candidate has exactly one `unsupported` representation and null selection. Its
content's MIME names duplicate the enclosing output's names and must agree.
Diagnostic references come from the enclosing output only, so adding unrelated
warnings or remapping indices cannot change the representation content digest.
The workspace projection has empty representation and
evidence vectors and exposes `unsupported_placeholder()`. These synthetic DTO
representations do not add workspace-v1 variants. Error/placeholder digests are
computed directly from their immutable record data.

Other outputs select their first accepted MIME alternative. An as-is stdout
rejection retains a literal fallback: its offered list still describes what was
offered to the converter and also contains `text/plain`, matching the current
reducer. A stream cannot become an unsupported display.

### Fragment tree and the decoded safety boundary

The following closed node forms freeze `FragmentBlock`/`FragmentInline`. They
retain the corresponding workspace field meanings with typed images. Every
node has `type` and `span`; the table lists its additional fields. No `code-cell`
variant is admitted, and no node registers authored targets.

| Block type | Additional fields |
| --- | --- |
| `paragraph` | `inlines:[Inline]` |
| `heading` | `level:1..6, attributes:Attributes, inlines:[Inline]` |
| `block-quote` | `blocks:[Block]` |
| `list` | `ordered:bool, items:[{checked:bool\|null,blocks:[Block],span:Span}]` |
| `thematic-break` | None. |
| `code-block` | `language:string\|null, source:string, source_segments:[SourceSegment]`; always inert. |
| `table` | `caption:[Inline], alignments:[default\|left\|center\|right], rows:[{header:bool,cells:[{blocks:[Block],span:Span}],span:Span}]` |
| `callout` | `kind:note\|tip\|important\|warning\|caution, attributes:Attributes, blocks:[Block]` |
| `unsupported` | `source_kind:string, raw:string`; escaped text only. |

| Inline type | Additional fields |
| --- | --- |
| `text`, `code` | `value:string` |
| `space`, `soft-break`, `hard-break`, `nonbreaking-space` | None. |
| `emphasis`, `strong`, `strikeout` | `inlines:[Inline]` |
| `link` | `inlines:[Inline], target:string, title:string\|null, attributes:Attributes` |
| `image` | `alt:[Inline], asset:AssetUse, title:string\|null, attributes:Attributes`. There is no `target` field on disk. |
| `auto-link` | `target:string` |
| `semantic-reference` | `target:string, target_span:Span` |
| `unsupported` | `source_kind:string, raw:string`; escaped text only. |

`Attributes` is exactly `{identifier:SpannedString|null,classes:[SpannedString],
key_values:[{key:SpannedString,value:SpannedString}]}`; a `SpannedString` is
`{value:string,span:Span}`. These describe inert source evidence. They do not
authorize arbitrary HTML attributes or generated anchors. The active fragment
validator checks that attribute uses follow the existing fragment policy.
Traverse every child field, including table captions/cells and image alt inlines.
Repeated image targets receive separate bindings even when assets deduplicate.
Without original fragment bytes, restore can check numeric bounds against the
declared byte length, node/range consistency, and valid decoded Unicode, but
cannot prove that every recorded offset was an original UTF-8 boundary. It must
not claim the stronger authored-source check or treat spans as trust evidence.

M6-07 decodes these forms without trusting them and constructs M6-05's
`DecodedMarkdown`/`DecodedHtml`. Restore checks the tree, links, image coverage,
and active policies against the current context and verified asset table. Any
rejection rejects the entire cache candidate, including unsafe unselected or
hidden alternatives. Cache never calls the live path-based constructors.

## Typed diagnostics and index ownership

An artifact diagnostic is exactly
`{code,severity,arguments,source,cell,slot,fragment,span,related_spans}`.
Severity is `warning`. Source is `{repository,path}` for page/cell diagnostics,
and null for generated fragments; the enclosing provenance identifies the page.
Cell and slot are nullable producer ordinals, never display IDs. Fragment is
null or `{ordinal,byte_length}`; fragment diagnostics require a producing cell.
Span is nullable; related spans are an ordered array in that same coordinate
space. Parser diagnostics use fragment ranges, not fabricated authored files.

`ExecutionDiagnostic` is a shared closed enum that owns the following argument
variants and attribution. Code/severity/displayed text are projections of this
enum. Emit it at the warning's source; do not parse `Diagnostic.message`.

| `arguments.kind` and additional fields | Code |
| --- | --- |
| `kernel-message-ignored` | `unsupported-kernel-message` |
| `unknown-display-update` | `unsupported-cell-output` |
| `no-supported-representation`, `mime_types:[string]` | `unsupported-cell-output` |
| `unsupported-media`, `media_type:string` | `unsupported-cell-output` |
| `invalid-mime-bundle` | `invalid-cell-output` |
| `invalid-text-payload`, `media_type:string` | `invalid-cell-output` |
| `invalid-image`, `media_type:string` | `invalid-cell-output` |
| `svg-rejected` | `unsafe-kernel-svg` |
| `html-rejected`, `reason:element\|attribute\|url\|remote-image\|structure` | `unsafe-kernel-html` |
| `markdown-rejected`, `reason:url\|remote-image\|structure` | `invalid-cell-output` |
| `fragment-unsupported`, `source_kind:string` | `unsupported-authored-syntax` |

Unsupported source kinds come directly from the node adapter. Current parser
errors are `invalid-embedded-yaml` with error severity; retain their parser
detail directly in the failure diagnostic, never by parsing a formatted
Diplodocus message. They cannot enter the successful-artifact warning catalog.
Only warnings can enter a successful artifact; parser error severity or fatal
output failures cannot be relabeled to make a successful cache record. Preparation and
discovery warnings are current-build diagnostics and are not in this catalog.
Likewise cache rejection/storage/nondeterminism and cleanup/failure diagnostics
are current-build observations, never replayed producing warnings. Fatal missing,
escaping, colliding, or unstaged assets retain their existing failure codes.

The engine owns an append-only execution warning ledger. Session receipt assigns
an internal event sequence; reducer emission uses that event's sequence and
within-event ordinal, with MIME candidates visited in policy order. At each
completed-cell reduction boundary, merge session and reducer emissions by
`(cell,event_sequence,within_event_ordinal)` and allocate stable ledger indices.
Readiness warnings precede cells; shutdown warnings follow them. No later sort
by message, code, or span is allowed. Internal transport sequence numbers never
enter the artifact. Remap each reducer-local output diagnostic index once to
its ledger index. Content digests do not cover diagnostic indices. Slot updates
reuse warning references but never allocate duplicate copies of a warning.

Portable result diagnostics are `current_before ++ execution_ledger ++
current_after`. `current_before` contains current preparation/discovery/cache
lookup diagnostics in their established order. Add its length to every output
diagnostic index exactly once; cache DTOs retain ledger-local indices. Current
publication warnings append in `current_after`. Do not mutate artifact-local
indices to render a build. Cache replay restores the execution ledger only.
On any later failure, carry the already accumulated current and execution
warnings into `ExecutionFailure.diagnostics` once, before the primary failure;
cleanup/rollback diagnostics remain in `cleanup_diagnostics`, in attempt order.

## Stable local inputs and one launch plan

```rust,ignore
// Coordinator/M6-06 construction; local data never enters portable serialization.
pub struct EngineInputs {
    pub repositories: BTreeMap<String, PathBuf>,
    pub configuration_directory: PathBuf,
    pub cache_root: PathBuf,
    pub build: BuildObservation,
}
pub struct PreparedExecution {
    /* private PageExecutionRequest, original source bytes, authored output context */
}
pub struct ResolvedLaunchPlan { /* private executable, argv/env templates, cwd */ }
// M6-04 accepts these local facts without importing SelectedKernel or KernelSession.
pub struct IdentityInputs {
    pub repositories: BTreeMap<String, PathBuf>,
    pub page: RepositoryFile,
    pub declared_files: Vec<RepositoryFile>,
    pub build: BuildObservation,
    pub launch: LaunchIdentityInput,
}
pub struct RepositoryFile { pub repository: String, pub path: DiagnosticPath }
pub async fn snapshot_inputs(
    inputs: IdentityInputs, request: &PageExecutionRequest,
    prepared_source: &[u8],
) -> Result<InputSnapshot, ExecutionFailure>;
impl InputSnapshot {
    pub fn identity(&self, runtime: &RuntimeObservation,
        request: &PageExecutionRequest, deadlines: &ExecutionDeadlines)
        -> Result<ExecutionIdentity, ExecutionFailure>;
    pub async fn revalidate(&self, launch: &LaunchIdentityInput)
        -> Result<(), ExecutionFailure>;
}
// Engine adapter only: the same plan supplies both views.
impl ResolvedLaunchPlan {
    pub fn identity_input(&self) -> LaunchIdentityInput;
    pub async fn spawn(&self, connection: &ConnectionFile)
        -> Result<KernelProcess, ExecutionFailure>;
}
```

`BuildObservation` contains exact engine version, running executable digest,
component-role records, and `ExecutionPlatform`. `RuntimeObservation` has the
five exact runtime fields in `key_input.kernel.runtime`. `LaunchIdentityInput`
contains the normalized private spec and launch records from the cache contract,
plus local executable/spec paths and resolver inputs needed for rereading.
It is nonserializable, with redacted Debug. Segments are literal, repository,
or connection-file, exactly as the existing vector defines. Environment entries
are sorted, unique explicit overrides; inherited environment values stay out.

Engine construction receives all declared canonical repository roots and a
configuration directory; default cache root is that directory joined with
`.diplodocus/cache/execution`. Caller staging remains a separate page boundary.
Store declared environment file identities separately from their snapshotted
fingerprints; callers cannot authorize cached digest assertions by populating
`request.declared_environment_inputs`. Snapshot reads the declared bytes itself
and checks agreement before constructing the final request identity.

Preparation binds source bytes, parsed request, and authored anchors immutably.
Snapshot rejects a page/source-digest mismatch or any prepared cell/options
mismatch before launch or submission. Do not accept a caller-mutated request
as evidence merely because its page digest matches; reprepare and compare, or
retain a privately constructed preparation token. Post-cleanup revalidation
rereads bytes and containment for source and every declared input, recomputes
selected spec/launch identity, and detects resolver changes. Changes produce
`execution-input-changed` without an automatic retry of authored code.

Resolve bare argv[0] once through the captured active search path. Store the
canonical executable path, its bytes/digest, actual argv[0], argument templates,
explicit environment templates, and cwd in the plan. Identity and spawn use
that same plan; spawn must not repeat PATH resolution or reuse the misleading
`SelectedKernel.executable_path` as a binary path. Substitute only validated
connection-file positions when spawning. Recheck executable identity immediately
before spawn and after cleanup. This detects ordinary file edits; it is not an
OS-level snapshot of interpreter dependencies or undeclared reads.

M6-04 obtains dependency versions from the selected locked build graph, not
Cargo requirement strings or the synthetic key fixture. The coordinator may
add build-time metadata generation when integrating 04: require exact selected
versions and `TARGET`, fail ambiguous/missing roles, and capture
`CARGO_PKG_VERSION` plus running executable bytes at runtime. Minimum roles are
the existing vector's eleven roles; include additional implementation components
under distinct roles when they affect behavior (for example HTML DOM, PNG/JPEG
codecs, URL parser, percent decoder, and SVG value parser). Built-in sanitizer
and image-validator names are stable Diplodocus adapter names with the crate
version, covered by the executable digest. Component sorting and duplicate-role
rejection remain unchanged. No version-probe kernel code is allowed.

## Cache work under session supervision

```rust,ignore
// M6-07: all request data is explicit; there is no kernel handle.
pub enum Lookup {
    Miss,
    Rejected(CacheWarning),
    Hit(RestoredCandidate),
}
pub async fn lookup(
    cache: &CacheRoot, identity: &ExecutionIdentity, prepared: &PreparedExecution,
    safety: &AuthoredOutputContext, assets: PageAssetStore,
) -> Result<Lookup, ExecutionFailure>;
pub async fn publish(
    cache: &CacheRoot, identity: &ExecutionIdentity, page: &ValidatedPage,
    assets: &[StagedExecutionAsset],
) -> Vec<CacheWarning>;
// M6-08: run inside the owner task, alongside cancellation and child exit.
impl ReadySession {
    pub async fn supervise<T, F>(
        &mut self, work: F, cancellation: &mut ExecutionCancellation<'_>,
    ) -> Result<T, ExecutionFailure>
    where F: Future<Output = Result<T, ExecutionFailure>> + Send, T: Send;
}
```

`RestoredCandidate` is nonserializable and provisional: validated content,
producing diagnostics, and candidate-owned staging, with no public conversion
to successful `PageExecutionResult`. Only the engine owner can accept it after
successful shutdown/reap, input revalidation, and final figure validation.
The session's cancellation receiver and child-liveness checks remain active
during lookup, asset reads/validation, and rollback. CPU validation is bounded
or performed through supervised work with an owned completion handle; canceling
a future may not leave a detached writer touching staging after rollback.

Lookup has a separate candidate staging owner. On miss/rejection, completely
rollback that owner before fresh execution in the same ready session. An invalid
artifact emits one stable `invalid-execution-cache` warning; absent or
incompatible schema is a normal miss. Rejection never retains a safe subset.
Staging/rollback failure is fatal, not an ordinary miss. Do not submit a cell
while candidate work or rollback still owns writable assets.
The caller creates a store exclusively for lookup and transfers it by value.
A hit returns that owner in `RestoredCandidate`; miss/rejection returns only
after consuming rollback. Fresh execution creates a new store. The owner task
retains the cancellation future across readiness, lookup, execution, and cleanup;
`supervise` borrows it instead of silently dropping the caller's signal.

On cancellation or child death, stop lookup, await its outstanding work, attempt
candidate rollback, then interrupt/shut down/reap as applicable, preserving the
first fatal failure and every cleanup/rollback failure. If cancellation and
success are both ready, observe cancellation before accepting success. The
owner task survives a dropped public future and completes cleanup. No cache
code discovers, starts, executes, or shuts down a kernel.

A hit submits zero cells; a miss/rejection submits once in that already-started
session. Publication runs only after cleanup, input revalidation, immutable
finalization, and staging retention. Nonblocking lock contention skips optional
publication; storage warnings leave the successful page usable. The cache
transaction remains separate from site publication.

## Reference fixture and implementation gates

[The canonical manifest](../spikes/fixtures/execution-artifact-v1/manifest.json)
is a complete synthetic artifact, with digest-named asset files. The adjacent
`source.qmd`, fragment sources, and README are fixture inputs, not cache entry
files. The fixture includes a slot gap, cross-cell update, a hidden Markdown-only
image asset, repeated nested image references, a semantic reference, HTML with
internal image binding, a stream, all representation tags, a placeholder warning,
an allowed error, and a skipped cell. Synthetic build/runtime values are not
observations of the installed development environment.

`tests/execution_artifact_contract.rs` independently checks canonical bytes and
digests against the existing key oracle, exact source preparation and options,
fragment trees against the current inert parser, asset bytes/closure, and final
warning/slot identity. These checks do not implement strict cache decoding or
grant rendering trust. M6-04 must reuse the original key oracle; M6-05 must add
live/restore rejection and compile-fail trust tests; M6-07 must reject unknown
fields, omitted nulls, forged references, altered source evidence, noncanonical
bytes, and any extra files in a real entry. M6-08 must test cancellation and
kernel death during lookup, failed rollback, zero-cell hits, and no stale fallback.
