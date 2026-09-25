# SQLite snapshot schema 2

This is the storage contract for a standalone Diplodocus snapshot. The [table
definitions](../../src/snapshots/schema.sql) are executable SQL used directly by
the [writer](../../src/snapshots/storage.rs). The JSON shapes below describe the
stored records, including the fields that the writer lifts out of the workspace
IR into separate rows.

## Versions

  | Manifest field     | Current value | Governs                                                                                                                                        |
  | ---                | ---           | ---                                                                                                                                            |
  | `storage_version`  | `2`           | SQLite tables, columns, keys, record kinds, splitting entities into rows, and storage-specific envelopes. Defined by `STORAGE_SCHEMA_VERSION`. |
  | `ir_version`       | `1`           | Semantic IR field shapes and meaning, including nested documents, signatures, and language extensions. Defined by `WORKSPACE_SCHEMA_VERSION`.  |
  | `encoding_version` | `1`           | Canonical record encoding, fingerprint inputs, and text export. Defined by `RECORD_ENCODING_VERSION`.                                          |

These versions are independent: a table-layout change requires a storage version
change; an incompatible signature or language-extension change requires an IR
version change even if the SQL tables stay the same. Changing canonical hashing
or export rules requires an encoding version change. A change may affect more
than one version. The current reader accepts only `(2, 1, 1)` and rejects any
unsupported component; it does not migrate snapshots automatically.

Storage version 2 adds the required `presentation` record. Version 1 snapshots
must be extracted again. The semantic IR and canonical encoding remain at
version 1 because presentation defaults are storage metadata outside the IR.

The workspace record also carries `schema_version: 1`, which must agree with
`manifest.ir_version`. Nested JSON inherits the containing snapshot's versions;
there is no additional version wrapper on every syntax node or signature.
Consumers must read the manifest before interpreting a detached record.
`producer` is the Diplodocus package version, not a schema discriminator.

`extract` replaces the complete snapshot, so deleted entities and unused assets
disappear on the next refresh.

The standalone database uses SQLite's `DELETE` rollback journal mode.
Publication writes a temporary sibling database in one transaction, commits and
closes it, synchronizes its contents, validates the completed artifact, and
replaces the destination with an atomic rename. Readers holding the previous
file can finish reading it, while new readers open the replacement. Failed
extraction, validation, writing, or replacement preserves the previous database.
Generation opens the database read-only and requires no source files, execution
cache, or language runtime.

Publication rejects a destination with a `-journal`, `-wal`, or `-shm` sidecar,
leaving both the database and sidecars untouched. Recovery state from a previous
database must never be applied to its replacement. Callers must close external
SQLite writers before publishing and keep them closed during publication. The
sidecar checks detect existing recovery files; they do not lock out an external
writer that starts concurrently.

## Tables

  | Table      | Key and cardinality                                                                        | Non-key columns                                                                                                          |
  | ---        | ---                                                                                        | ---                                                                                                                      |
  | `manifest` | `id INTEGER PRIMARY KEY CHECK (id = 1)`; exactly one row                                   | `storage_version INTEGER`, `ir_version INTEGER`, `encoding_version INTEGER`, `producer TEXT`, `content_fingerprint TEXT` |
  | `records`  | `PRIMARY KEY (kind, owner, id)`; all three columns are `TEXT`; one row per semantic record | `content TEXT`, `fingerprint TEXT`                                                                                       |
  | `assets`   | `digest TEXT PRIMARY KEY`; one row per distinct content digest                             | `media_type TEXT`, `bytes BLOB`                                                                                          |

All non-key columns and all text key columns are `NOT NULL`. `records` and
`assets` use `WITHOUT ROWID`; the schema declares no secondary indexes, views,
triggers, or foreign keys. The primary key supports exact lookups and scans by
record kind. Text keys use SQLite's default binary collation and are
case-sensitive. Empty owner and ID sentinels are empty strings, never SQL NULL.

`content` is a UTF-8 JSON object. `fingerprint`, `content_fingerprint`, and
`digest` are lowercase, 64-character SHA-256 hex strings. `bytes` contains the
exact asset bytes, and `media_type` identifies their validated image format or
`application/octet-stream` for a download. SQL enforces the declared keys and
nullability. The snapshot loader enforces record shapes, cross-record
relationships, and content integrity; successful SQL insertion alone does not
establish a valid snapshot.

## Record keys and fields

An ID is the key from the corresponding workspace map, not a display name,
qualified lookup name, source filename, or rendered URL. Items and targets are
scoped to a package. All other entity IDs are workspace-scoped. The [semantic
identity contract](../ir/item-identity.md) defines item keys, including distinct
callable-family and overload identities. Slugs and content mounts remain
presentation fields and do not replace these keys.

The `records` kinds are:

  | Kind         | Owner      | ID                            | Content                                                                                                                                              |
  | ---          | ---        | ---                           | ---                                                                                                                                                  |
  | `workspace`  | Empty      | Empty                         | `schema_version`, `name`, `relationships`, `diagnostics`, `provenance`                                                                               |
  | `presentation` | Empty    | Empty                         | `title`, `description`                                                                                                                            |
  | `repository` | Empty      | Repository ID                 | `canonical_url`, `source_link_template`, `revision`, `dirty`, `declared_input_fingerprint`                                                           |
  | `package`    | Empty      | Package ID                    | `slug`, `name`, `ecosystem`, `version`, `repository`, `path`, `metadata_path`, `kind`, `visibility`                                                  |
  | `target`     | Package ID | Target ID                     | `extractor`, `path`, `role`                                                                                                                          |
  | `item`       | Package ID | Semantic item ID              | `kind`, `name`, `qualified_name`, optional `language_data` and `aliases`, `signatures`, `documentation`, `source_location`, `children`, `provenance` |
  | `collection` | Empty      | Collection ID                 | `owner`, `repository`, `path`, `mount`, `format`, `execution`                                                                                        |
  | `page`       | Empty      | Semantic page ID              | `owner`, `kind`, `title`, `document`                                                                                                                 |
  | `concept`    | Empty      | Concept ID                    | `kind`, `members`, `documentation`                                                                                                                   |
  | `document`   | Empty      | Serialized `DocumentIdentity` | `document`, `collection_path`, `anchors`, `references`                                                                                               |
  | `execution`  | Empty      | Page ID                       | `record`, `diagnostics`, `diagnostic_offset`, `slots`                                                                                                |

There is exactly one `workspace` record and one `presentation` record. Every
entity map entry becomes one row, with its map key in `id`; the JSON content
does not duplicate that ID. The
workspace row omits `repositories`, `packages`, `content_collections`, `pages`,
and `concepts`. Package rows omit `items` and `extraction_targets`. Loading
rebuilds those maps from their rows, including empty maps. All entity IDs and
package owners must be nonempty; only the workspace and presentation IDs are
empty.

Each page has exactly one `document` row. An item or concept has one precisely
when its `documentation` is non-null. This row stores resolution metadata; the
syntax tree stays inside the owning page, item, or concept. Its ID is the
compact Serde encoding of `DocumentIdentity`, with these exact member orders:

- `{"kind":"page","page":"<page ID>"}`
- `{"kind":"item","item":{"package":"<package ID>","item":"<item ID>"}}`
- `{"kind":"concept","concept":"<concept ID>"}`

Angle-bracketed values here stand for JSON-escaped strings. These ID strings are
opaque keys: do not sort their embedded JSON members when computing record
fingerprints. In contrast, the `document` object inside `content` uses the
ordinary canonical object-key order. Queries can inspect that object instead of
constructing the serialized key.

An authored page processed by authorized execution has one `execution` row keyed
by its page ID, including a result with only skipped cells. Pages assembled
without execution have none. Relationships, diagnostics, aliases, signatures,
and provenance have no independent IDs; they remain nested arrays or objects.

## Relationships

References in JSON are semantic joins, not SQL foreign keys. The following table
defines their targets. External package coordinates remain provenance and do not
require a repository or package row.

  | Referencing field                                                          | Target or invariant                                                                                                            |
  | ---                                                                        | ---                                                                                                                            |
  | `package.repository`, `collection.repository`, `SourceLocation.repository` | `('repository', '', repository)`                                                                                               |
  | `item.owner`, `target.owner` in SQL                                        | `('package', '', owner)`                                                                                                       |
  | `ContentOwner` with `kind: "package"`                                      | `('package', '', package)`; `kind: "project"` means workspace ownership                                                        |
  | `page.kind` with `kind: "authored"`                                        | `('collection', '', collection)`; page and collection owners agree                                                             |
  | `page.kind` with `kind: "api"` or `kind: "concept"`                        | The referenced item or `('concept', '', concept)`, respectively; `overview` has no additional target                           |
  | `ItemReference {package, item}`                                            | `('item', package, item)`, including concept members, signature targets, and language-specific family links                    |
  | `item.children[]`                                                          | `('item', owning package, child ID)`                                                                                           |
  | `TargetReference {package, target}` in extraction provenance               | `('target', package, target)`                                                                                                  |
  | `workspace.relationships[].from` and `.to`                                 | `{"kind":"workspace","package":ID}` joins a package; `{"kind":"external","ecosystem":E,"name":N}` retains external coordinates |
  | `document.document`                                                        | Owning page, item, or concept with a structured document                                                                       |
  | `ReferenceTarget` with `kind: "item"` or `kind: "page"`                    | Referenced item or page; a nonempty page `fragment` names an authored anchor                                                   |
  | `ReferenceTarget` with `kind: "anchor"`                                    | `fragment` names an anchor in the containing document                                                                          |
  | `ReferenceTarget` with `kind: "asset"`                                     | `asset.fingerprint.value` joins `assets.digest`; `fragment` is nullable                                                        |
  | `ReferenceTarget` with `kind: "external"`                                  | `url` is an accepted external hyperlink, not a local entity                                                                    |
  | Execution row `id`                                                         | `('page', '', id)`; execution collection, source, and policy agree with that authored page                                     |
  | `AssetReference` in documents or execution records                         | `fingerprint.algorithm` is `sha256`; `fingerprint.value` joins `assets.digest`                                                 |

`AssetReference` is
`{"path": relative_path, "fingerprint": {"algorithm": "sha256", "value": digest}}`.
The path is a portable output reference; the digest selects the bytes. Multiple
references can share one asset row. `SourceLocation` is
`{"repository": ID, "path": relative_path, "span": span}`. Source locations
provide evidence and never require generation to open files.

### Asset retention

Checked-in images, local downloads, and generated figures share the `assets`
table. The digest hashes the exact bytes, without transcoding. Identical bytes
occupy one row even when references come from different files, pages, or
execution results. A download uses `application/octet-stream`; if the same
bytes also appear as a validated image, the row retains the image's media type.

Resolved document references use `content-assets/sha256/<digest>` for
checked-in files and retain any link fragment separately. Execution references
use `execution-assets/<page>/sha256/<digest>`. These paths identify portable
references, not source or cache files. Both forms recover their bytes by joining
`fingerprint.value` to `assets.digest`. Execution records also retain media type,
byte size, and the association with each output representation, including
hidden output and unselected MIME alternatives.

`Snapshot::from_sources` and `Snapshot::from_executed` own the collected bytes.
After construction, the source checkout, execution cache, and staging directory
can be removed. Publication writes the bytes into the database, and loading
recovers them through `Snapshot::assets` without those directories. Generation
maps the retained references to local asset URLs and writes the original bytes.

## JSON shapes

The field tables describe canonical writer output. Strings, booleans, integers,
arrays, and objects retain their JSON types. Nullable fields serialize as JSON
`null`, not SQL NULL. Empty arrays and maps remain `[]` and `{}`. The writer
omits `item.language_data` when absent, `item.aliases` when empty, and code-cell
`outputs` when empty. Diagnostic omissions are described below. Other fields in
the record table remain present. Decoding alone can accept some omitted optional
fields; the loader's canonical re-encoding check requires the writer's shape.

### Presentation defaults

The `presentation` singleton retains the optional `[presentation]` configuration
independently of semantic workspace identities:

```json
{"title": "Foo documentation", "description": "Documentation for Foo."}
```

Both fields are nullable strings and are always present in storage, even when
configuration omits the table. A null `title` uses `workspace.name`; a null
`description` omits HTML description metadata. The title supplies site branding,
the page-title suffix, and the generated project overview title. Authored page
titles remain part of their documents. Generation treats these values as text
and escapes them for HTML. The initial generator uses its built-in theme.
Local theme paths, output directories, checkout roots, and execution authority
are not presentation defaults. Unknown fields and malformed values are rejected.

`Snapshot::presentation()` exposes these defaults, and `Snapshot::producer()`
returns the original manifest producer version without replacing it with the
loading binary's version. Repository URLs, source-link templates, revision,
dirty state, and input fingerprints remain in repository records. Unknown Git
observations remain null; configured revisions take precedence over observed
HEAD. Diagnostic messages, entities, relative source paths, spans, and related
spans stay in the workspace record. Producer-specific parser versions and input
fingerprints remain in each record's provenance. Loading needs none of the
original repositories or configuration files.

### Semantic records

The [workspace types](../../src/ir/workspace.rs) define all entity fields.
Repository metadata fields are nullable strings, except `dirty` (nullable
boolean) and `declared_input_fingerprint` (nullable `Fingerprint`). Package
`version` is a nullable string. Package kinds are `package` and `component`;
visibility is `public`, `internal`, or `hidden`. Item kinds are `module`,
`function`, `method`, `type`, `class`, `constant`, `field`, and `namespace`.

Paths are normalized, forward-slash relative strings. A nullable root `path`
means the repository or package root; `metadata_path` is always present. Page
`owner` and collection `owner` use the tagged `ContentOwner` objects above.
Collection `format` is `qmd` or `gfm`. Its `execution` object has `mode`
(`never` or `execute`), nullable `engine` (`jupyter`), nullable `kernel`
(string), and `declared_environment_inputs` (sorted repository-relative paths).
These are declarations, not authority to execute during loading or generation.

Package slugs and nonempty content mounts must also be normalized relative
paths. They cannot contain control characters or URL query, fragment, or
percent-encoding delimiters. An empty mount places content at its owner's root.

Concept `kind` is `equivalent`, `analogous`, or `related`; `members` is a
sorted, unique array of `{package, item}` objects. A package relationship
contains `from`, `to`, `kind`, nullable `version_constraint`, and `provenance`.
Its `kind` is `depends-on`, `binds`, `wraps`, or `generated-from`.

`Provenance` contains `activity`, nullable `source`, nullable `span`, and
`tools` (an object mapping component names to versions). `activity.kind` selects
`declaration`, `extraction`, `execution`, or `generated-markdown`; the
[provenance types](../../src/ir/provenance.rs) specify each variant's evidence.
`Fingerprint` contains `algorithm` and `value`. Diagnostics use the shared
[diagnostic types](../../src/diagnostics.rs): `code`, `severity`, `message`, and
nullable `span` are always present. `related_entity` and `source` are omitted
when absent, and `related_spans` is omitted when empty.

Maps sort by key. Sets serialize as sorted, unique arrays, including concept
members, anchors, diagnostics, and declared environment inputs. Other arrays
retain semantic order: relationships follow declaration order, signatures and
parameters follow declaration order, document blocks follow source order, and
resolved references follow depth-first traversal order. Sorting these arrays
would change their meaning.

### Sourced document example

Pages store a `SourcedDocument` in `document`; items and concepts use nullable
`documentation`. The envelope keeps the tree, format, location, optional raw
source, and provenance together:

```json
{
  "document": {
    "span": {"start": 0, "end": 6},
    "frontmatter": null,
    "blocks": [{
      "type": "paragraph",
      "inlines": [{"type": "text", "value": "Hello.", "span": {"start": 0, "end": 6}}],
      "span": {"start": 0, "end": 6}
    }]
  },
  "source_format": {"kind": "authored", "format": "gfm"},
  "source_location": {"repository": "core", "path": "docs/hello.md", "span": null},
  "raw_source": "Hello.",
  "provenance": []
}
```

`source_format` can instead be `{"kind":"extracted","name":"rd"}` or
`{"kind":"generated"}`. The [document tree](../../src/ir.rs) uses `type`
discriminators for blocks, inlines, and metadata, with kebab-case variant names.
Spans are zero-based, half-open byte ranges `{start, end}`. Tree spans are
relative to document source; a source-location span refers to its repository
file. `frontmatter` is a nullable typed metadata tree, not a free-form JSON map.
Code cells retain options and structured outputs; [output
types](../../src/ir/outputs.rs) distinguish plain text, Markdown, HTML
candidates, and asset references.

### Sourced signature example

Every entry of `item.signatures` pairs a structured signature with independent
source evidence. Defaults and annotations retain syntax without evaluating it:

```json
{
  "signature": {
    "kind": "callable",
    "parameters": [{
      "name": "x",
      "kind": {"kind": "positional-or-keyword"},
      "annotation": {"kind": "name", "name": "int", "target": null},
      "default": {"kind": "literal", "text": "1"}
    }],
    "returns": null
  },
  "sources": [{
    "source": {"repository": "python", "path": "src/foo.pyi", "span": null},
    "role": "signature",
    "parsers": ["ruff"]
  }]
}
```

The [signature types](../../src/ir/signatures.rs) also define `value` signatures
(`annotation`, `value`) and `language-specific` signatures (`language`,
`syntax`). A parameter has `name`, tagged `kind`, nullable `annotation`, and
nullable `default`. Its kind can be `positional-only`, `positional-or-keyword`,
`keyword-only`, `variadic-positional`, `variadic-keyword`, or
`language-specific` (with `language` and `name`). A null default means no
default, whereas `{"kind":"literal","text":"None"}` retains an actual default
value.

Expressions use `kind`: `name` has `name` and nullable `target`
(`ItemReference`); `literal` has `text`; `apply` has `constructor` and
`arguments`; `sequence` has `items`; and `language-specific` has `language`,
`name`, `children`, and nullable `source`. Recursive child expressions remain
structured JSON.

### Python language extension example

`item.language_data` uses a `language` discriminator and a `data` object. A
Python callable family with no overloads is:

```json
{
  "language": "python",
  "data": {
    "visibility": "public",
    "declaration": {
      "kind": "callable",
      "binding": "function",
      "is_async": false,
      "role": {"kind": "family", "overloads": []}
    },
    "decorators": []
  }
}
```

### R language extension example

An ordinary exported R function is:

```json
{
  "language": "r",
  "data": {"exported": true, "declaration": {"kind": "function"}}
}
```

The [language types](../../src/ir/languages.rs) define all tagged declarations:
Python modules, callables, classes, properties, constants, fields, and type
aliases; R functions, S3 generics, S3 methods, and constructors. Family,
overload, constructor, and method links use `ItemReference` rather than names or
URLs. R generic references explicitly distinguish workspace items from external
package coordinates. Language extensions inherit `ir_version`; they are typed
records, not an unversioned extension bag. `aliases` entries contain
`qualified_name`, `kind`, and `sources`, and bind additional lookup names to the
owning item without creating new identities.

### Resolution and execution records

`document` content follows
[ResolvedDocument](../../src/validation/workspace.rs): `document` is the tagged
identity, `collection_path` is a nullable collection-relative filename,
`anchors` is a sorted array of authored anchor strings, and `references` is an
ordered array of `{kind, spelling, target}`. Reference kinds are `semantic`,
`link`, and `image`; targets use the tagged objects described under
relationships. Targets remain portable semantic identities until generation
assigns routes.

The storage-specific [execution envelope](../../src/snapshots/outputs.rs) has
`record` ([PageExecutionRecord](../../src/execution/records.rs)), `diagnostics`
(typed execution diagnostics), `diagnostic_offset` (their starting index in the
record's diagnostic list), and `slots`. Each slot has integer `owning_cell` and
`slot` ordinals, `origin` (producer attribution), and ordered `representations`.
Each representation uses `kind`: `text` has `text`; `markdown` has `content` and
`origin`; `html` has `content`; `asset` has `asset` (portable execution image
metadata). Markdown content retains an inert fragment tree and its bindings;
HTML content is `{"markup": string}`; sanitizer evidence stays in the portable
output record. The [output safety types](../../src/execution/output_safety.rs)
define their complete shapes. All accepted alternatives are stored, including
hidden output.

These values carry no trust when decoded. The loader binds source preparation,
document projection, diagnostic associations, and representations again using
the shared execution validators. It verifies every image from the database's
bytes and revalidates every Markdown and HTML alternative, including hidden
output and unselected alternatives, without writing staging files.

## Query examples

### Package metadata

Query all package names and versions:

```sql
SELECT id,
       json_extract(content, '$.name') AS name,
       json_extract(content, '$.version') AS version
FROM records WHERE kind = 'package' ORDER BY id;
```

### Item lookup

Query an API item by its package and semantic identity. Bind `?1` to the package
ID and `?2` to the semantic item ID, for example `pyfoo` and
`sid1:python:function:foo.model.fit` in the acceptance workspace. Its re-export
name `foo.fit` is an alias, not a second record key:

```sql
SELECT content, fingerprint
FROM records WHERE kind = 'item' AND owner = ?1 AND id = ?2;
```

### Item documentation

Join the resolution record back to its item using semantic keys:

```sql
SELECT i.owner, i.id, json_extract(d.content, '$.references') AS references_json
FROM records AS i
JOIN records AS d
  ON d.kind = 'document' AND d.owner = ''
 AND json_extract(d.content, '$.document.kind') = 'item'
 AND json_extract(d.content, '$.document.item.package') = i.owner
 AND json_extract(d.content, '$.document.item.item') = i.id
WHERE i.kind = 'item' AND i.owner = ?1 AND i.id = ?2;
```

### Resolved documents

Documents retain semantic reference targets rather than site URLs:

```sql
SELECT id, json_extract(content, '$.references') AS references_json
FROM records WHERE kind = 'document' ORDER BY id;
```

### Asset lookup

Read retained asset metadata without consulting a source checkout:

```sql
SELECT digest, media_type, length(bytes) AS byte_size
FROM assets ORDER BY digest;
```

## Canonical encodings

The [canonical encoder](../../src/snapshots/canonical.rs) implements
`encoding_version = 1` for both the database and text export. The following
rules are part of that version:

- Sort JSON object keys recursively by their unescaped UTF-8 string order,
  including objects nested in arrays. This does not depend on the JSON map
  implementation or insertion order.
- Serialize typed maps and sets in their IR-defined order. String maps and sets
  use lexical order, concept members use `(package, item)`, and diagnostics use
  the [common diagnostic ordering](../../src/diagnostics.rs). Sets do not retain
  insertion order or duplicate entries.
- Preserve arrays representing meaningful order, including document nodes,
  signatures, parameters, child items, aliases, relationships, references,
  provenance, execution events, and MIME alternatives. Do not sort arbitrary
  JSON arrays; typed sets already supply their canonical order.
- Preserve strings without Unicode normalization, newline conversion, or
  interpretation as embedded JSON. This includes opaque semantic IDs and a
  document record's serialized ID. Absent fields and explicit `null` values
  retain the distinction specified by their typed record shapes.
- Use `serde_json`'s compact UTF-8 serialization of the sorted JSON value for
  hashing, with no byte-order mark, padding, or trailing newline. Strings escape
  quotes, backslashes, and control characters; other Unicode characters remain
  UTF-8. Integers use decimal notation. The exact serialization, including any
  future change to number or string formatting, belongs to the encoding version.
- Encode SHA-256 digests as 64 lowercase hexadecimal characters.

A record fingerprint hashes the compact JSON array
`["diplodocus/snapshot-record-v1", key, content]`. The key object contains
`kind`, `owner`, and `id`; `content` is the complete record object from the
table above. The digest excludes its own `fingerprint` field. Including the
domain tag and key distinguishes otherwise identical content belonging to
different entities. For example, the following exact UTF-8 bytes, without the
code block's final newline, hash to
`2d37832c264aa2932e3cf22bcc96f9e340367c5912fae1f78ad39df06bc6914c`:

```json
["diplodocus/snapshot-record-v1",{"id":"","kind":"presentation","owner":""},{"description":null,"title":"Canonical café"}]
```

Each entity's fingerprint covers its own row, not the transitive contents of
referenced entities. For example, editing an item changes its `item` fingerprint
without changing its owning `package` fingerprint. A future incremental
generator must follow dependencies separately. Portable provenance is part of
the record: revisions, dirty state, tool versions, and fresh/cache execution
origin can change a fingerprint even when rendered documentation stays the same.

The logical snapshot object has exactly `storage_version`, `ir_version`,
`encoding_version`, `producer`, `records`, and `assets`. Records contain `kind`,
`owner`, `id`, `content`, and `fingerprint`, sorted by `(kind, owner, id)`.
Assets contain `digest`, `media_type`, and integer `byte_size`, sorted by digest.
The manifest's `content_fingerprint` hashes this entire object with the compact
encoding above. It has no additional domain prefix or self-referential digest
field. An asset's `digest` separately hashes its exact bytes without an envelope.
SQLite page size, row insertion order, JSON whitespace in SQL values, indexes,
journal state, and local checkout roots do not enter these hashes.

`Snapshot::canonical_export` returns the same logical object with one extra
`bytes_base64` field on each asset. It uses the standard padded base64 alphabet
without line wrapping, recursively sorted object keys, two-space JSON
indentation, LF line endings, and exactly one trailing newline. This is a
readable export, not an import format. For example:

```rust
let snapshot = diplodocus::snapshots::Snapshot::load("documentation.sqlite")?;
std::fs::write("documentation.json", snapshot.canonical_export()?)?;
```

Comparing exports establishes logical equivalence, including portable metadata
and producer version, independently of physical database layout. Removing each
asset's `bytes_base64` field and applying the compact encoding recovers the
manifest's fingerprint input; hashing the pretty export does not. The
[minimal export fixture](../../tests/snapshots/snapshot-encoding/minimal.json)
pins this format and has logical snapshot fingerprint
`a573096448832e533ef78a5f1dcef56a388dd7735eb08955438f6e3403bf504f`.

Fingerprints provide integrity checks, not authentication: loading also checks
record sets, semantic references, anchors, paths, asset media, and active output
policies.
Reference validation includes nested signature expressions, Python and R
language records, source evidence, and provenance. Source repository and
extraction-target IDs must resolve within the snapshot, but source files are
never opened. External package and R generic coordinates do not require local
entities. Cell-option declaration indices must select options with the same
canonical key before document anchors or execution outputs are traversed.

## Contract checks

[Canonical encoding tests](../../src/snapshots/canonical/tests.rs) pin record
and snapshot digests computed independently with Python's sorted compact JSON
encoding and `hashlib.sha256`, as well as the readable export fixture. They
check identity binding, Unicode and control characters, array order, unordered
collections, and isolated entity changes. [Canonical export
tests](../../tests/snapshot_canonical.rs) retain binary asset bytes and compare
exports after changing SQLite page size, reinserting records and assets in
reverse order, and rewriting JSON object order and whitespace without updating
stored fingerprints. Run both JSON map backends:

```sh
cargo test --locked --lib snapshots::canonical::tests
cargo test --locked --test snapshot_canonical
cargo test --locked --lib snapshots::canonical::tests --features serde_json/preserve_order
cargo test --locked --test snapshot_canonical --features serde_json/preserve_order
```

[Schema contract tests](../../tests/snapshot_schema.rs) run the queries above
against a published Python/R workspace, check every stored entity against its IR
map key and fields, and round-trip the JSON examples through their typed IR
decoders. [Storage tests](../../tests/snapshots_storage.rs) compare loaded IR,
resolved documents, and assets with the original assembly and resolution results
after removing the source checkout. They distinguish malformed JSON from record,
asset, and manifest fingerprint failures, and cover refreshes and execution
restoration.
[Publication tests](../../tests/snapshot_publication.rs) verify standalone copies,
readers retaining the previous complete database across replacement, and failed
refreshes with existing sidecars or active SQLite transactions. A storage unit
test verifies that rejection of a completed staging database leaves the previous
snapshot intact and removes the temporary file.
[Asset handoff tests](../../tests/snapshot_assets.rs) check exact SQLite bytes,
deduplication across checked-in and generated assets, download fragments, and
PNG, JPEG, and SVG recovery from a copied database after removing the checkout,
execution cache, and staging directory. They also restore hidden figure
alternatives from a confirmed cache hit and render the recovered assets.
[Validation tests](../../tests/snapshot_validation.rs) recompute record and
snapshot fingerprints after introducing malformed records, dangling references,
invalid paths, and missing assets, so checksum failures cannot mask validation
gaps. Image tests also recompute asset digests and references before testing
truncated SVG, PNG, and JPEG contents, mismatched media types, and active SVG
content. A valid replacement image verifies that the mutation helper preserves
loadable records. Rejected loads leave database bytes unchanged. Version tests
reject old and future storage, IR, and encoding versions before reading records
or assets, without migration. Run the full snapshot suite with:

```sh
cargo test --locked --lib snapshots::
cargo test --locked --test snapshot_schema --test snapshots_storage \
  --test snapshot_assets --test snapshot_validation --test snapshot_metadata \
  --test snapshot_canonical --test snapshot_publication
```

Use the project's devenv shell, which supplies the execution tests' Python
kernel. CI also runs this suite with `--features serde_json/preserve_order` to
exercise validation and round trips with insertion-ordered JSON maps.

## Generation boundary

The loader returns an immutable `Snapshot`. `Site::new` assigns
collision-checked routes and prepares cell visibility from the retained
declarations. Rendering consumes that model and its verified output wrappers,
escapes ordinary text, chooses the first accepted MIME representation, and maps
asset digests to local URLs. It never opens a database or source checkout. The
output contains local styles, search data and behavior, and all referenced asset
bytes.

Site publication stages a sibling directory. On Linux, an existing site is
replaced with an atomic directory exchange. Other platforms use a backup rename
with rollback. Publication accepts an empty destination or a tree marked by an
earlier Diplodocus publication. Commands also reject destinations that would
replace selected inputs or the input snapshot.
